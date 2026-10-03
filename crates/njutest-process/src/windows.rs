// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Windows supervision: a Job Object per child.

use std::io;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::Instant;

use windows_sys::Win32::Foundation::{
    HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Pipes::{PIPE_NOWAIT, PeekNamedPipe, SetNamedPipeHandleState};
use windows_sys::Win32::System::SystemServices::JOB_OBJECT_MSG_NEW_PROCESS;
use windows_sys::Win32::System::Threading::{CREATE_SUSPENDED, WaitForSingleObject};

use super::LeaderObservation;

/// Whether a read of no bytes was the end of the stream rather than a pipe with nothing in it yet.
///
/// A `PIPE_NOWAIT` handle reads an empty pipe as a success of no bytes, and the standard library turns the broken pipe that says the writers are gone into that same answer, so the two are one value here and only the kernel can tell them apart.
///
/// # Errors
/// The operating system refused this owned process transition.
pub fn stream_ended(reader: &io::PipeReader) -> io::Result<bool> {
    let mut available: u32 = 0;
    #[expect(
        unsafe_code,
        reason = "PeekNamedPipe is the Windows boundary for asking whether a pipe still has writers"
    )]
    let peeked = unsafe {
        PeekNamedPipe(
            reader.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(available),
            std::ptr::null_mut(),
        )
    };
    if peeked == 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::BrokenPipe {
            return Ok(true);
        }
        return Err(error);
    }
    Ok(false)
}

/// Makes an anonymous pipe reader pollable so its owner can cancel and join it even when a descendant retained the write handle.
///
/// # Errors
/// The operating system refused this owned process transition.
pub fn configure_reader(reader: &io::PipeReader) -> io::Result<()> {
    let mode = PIPE_NOWAIT;
    #[expect(
        unsafe_code,
        reason = "SetNamedPipeHandleState is the Windows boundary for a cancellable pipe reader"
    )]
    let configured = unsafe {
        SetNamedPipeHandleState(
            reader.as_raw_handle(),
            std::ptr::addr_of!(mode),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if configured == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The status `TerminateJobObject` stamps on every process in the job.
/// It never reaches a caller.
const TERMINATED_JOB_EXIT_CODE: u32 = 1;

/// Owns a Windows Job Object holding one child process tree.
#[derive(Debug)]
pub(crate) struct Supervisor {
    job: Option<OwnedHandle>,
    membership: std::sync::Arc<Membership>,
}

/// The completion port a job posts to, from which every process that ever ran in it can be named.
#[derive(Debug)]
pub struct Membership {
    port: OwnedHandle,
    queue: Mutex<Vec<u32>>,
}

impl Membership {
    fn new() -> io::Result<Self> {
        #[expect(unsafe_code, reason = "CreateIoCompletionPort has no safe binding")]
        let port =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
        if port.is_null() {
            return Err(unavailable(
                "could not create the completion port a job names its processes on",
            ));
        }
        #[expect(
            unsafe_code,
            reason = "the newly created completion port transfers exactly once into its safe handle owner"
        )]
        let port = unsafe { OwnedHandle::from_raw_handle(port) };
        Ok(Self {
            port,
            queue: Mutex::new(Vec::new()),
        })
    }

    /// Every process the job has said it took in since the last time it was asked, without waiting for more.
    /// Windows does not promise to post every one, so a process missing here is unknown rather than absent.
    #[must_use]
    pub fn drain(&self) -> Vec<u32> {
        let mut named = match self.queue.lock() {
            Ok(named) => named,
            Err(source) => {
                super::group::terminal(&format!("job event ownership was poisoned: {source}"))
            }
        };
        loop {
            match self.receive(0) {
                Ok(Some((message, detail))) => retain_member(&mut named, message, detail),
                Ok(None) => return std::mem::take(&mut *named),
                Err(source) => {
                    super::group::terminal(&format!("job membership observation failed: {source}"))
                }
            }
        }
    }

    fn receive(&self, milliseconds: u32) -> io::Result<Option<(u32, usize)>> {
        let (mut message, mut key, mut detail) =
            (0_u32, 0_usize, std::ptr::null_mut::<OVERLAPPED>());
        #[expect(
            unsafe_code,
            reason = "GetQueuedCompletionStatus receives events from the owned port under its single-consumer lock"
        )]
        let received = unsafe {
            GetQueuedCompletionStatus(
                self.port.as_raw_handle(),
                std::ptr::addr_of_mut!(message),
                std::ptr::addr_of_mut!(key),
                std::ptr::addr_of_mut!(detail),
                milliseconds,
            )
        };
        if received != 0 {
            return Ok(Some((message, detail.addr())));
        }
        let source = io::Error::last_os_error();
        let timeout = i32::try_from(WAIT_TIMEOUT).map_err(io::Error::other)?;
        if source.raw_os_error() == Some(timeout) {
            Ok(None)
        } else {
            Err(source)
        }
    }
}

fn retain_member(named: &mut Vec<u32>, message: u32, detail: usize) {
    if message == JOB_OBJECT_MSG_NEW_PROCESS {
        match u32::try_from(detail) {
            Ok(pid) => named.push(pid),
            Err(source) => super::group::terminal(&format!(
                "the job membership PID exceeded its kernel width: {source}"
            )),
        }
    }
}

type NtResumeProcess = unsafe extern "system" fn(HANDLE) -> i32;

fn unavailable(message: &str) -> io::Error {
    let source = io::Error::last_os_error();
    io::Error::new(source.kind(), format!("{message}: {source}"))
}

impl Supervisor {
    /// Creates and configures the job object before the child exists, so a machine that cannot create job objects is discovered while nothing is running.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn new() -> io::Result<Self> {
        #[expect(unsafe_code, reason = "CreateJobObjectW has no safe binding")]
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(unavailable(
                "could not create the job object that owns the child process tree",
            ));
        }
        #[expect(
            unsafe_code,
            reason = "the newly created job transfers exactly once into its safe handle owner"
        )]
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let supervisor = Self {
            job: Some(job),
            membership: std::sync::Arc::new(Membership::new()?),
        };
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: supervisor.job()?,
            CompletionPort: supervisor.membership.port.as_raw_handle(),
        };
        let association_size = u32::try_from(size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>())
            .map_err(io::Error::other)?;
        #[expect(unsafe_code, reason = "SetInformationJobObject has no safe binding")]
        let associated = unsafe {
            SetInformationJobObject(
                supervisor.job()?,
                JobObjectAssociateCompletionPortInformation,
                std::ptr::addr_of!(association).cast(),
                association_size,
            )
        };
        if associated == 0 {
            return Err(unavailable(
                "could not have the job object name its processes on a completion port",
            ));
        }
        #[expect(
            unsafe_code,
            reason = "the Windows structure is initialized through its documented zero state"
        )]
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(io::Error::other)?;
        #[expect(unsafe_code, reason = "SetInformationJobObject has no safe binding")]
        let ok = unsafe {
            SetInformationJobObject(
                supervisor.job()?,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                size,
            )
        };
        if ok == 0 {
            return Err(unavailable(
                "could not configure the job object to kill its processes on close",
            ));
        }
        Ok(supervisor)
    }

    /// The child is created suspended, so that it can be assigned to the job before it has run an instruction: a process that has never executed cannot have forked.
    #[expect(
        clippy::unused_self,
        reason = "the same signature as the unix supervisor, which configures from the group it holds"
    )]
    /// Preserves the owned platform observation through this transition.
    pub(super) fn configure(&self, command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    /// Assigns the suspended child to the job and resumes it.
    /// Before assignment the caller owns the suspended child; after assignment this supervisor's Job Object owns it even when resuming fails.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn adopt(&mut self, child: &Child) -> io::Result<()> {
        let process: HANDLE = child.as_raw_handle();
        #[expect(unsafe_code, reason = "AssignProcessToJobObject has no safe binding")]
        let assigned = unsafe { AssignProcessToJobObject(self.job()?, process) };
        if assigned == 0 {
            return Err(unavailable(
                "could not assign the child process to the job object",
            ));
        }
        let resume = resume_entry_point()?;
        #[expect(unsafe_code, reason = "calling NtResumeProcess is the FFI boundary")]
        let resumed = unsafe { resume(process) };
        if resumed != 0 {
            return Err(io::Error::other(
                "NtResumeProcess refused to resume the suspended child",
            ));
        }
        Ok(())
    }

    /// Where this job names the processes it holds.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the same signature as the unix supervisor, whose group names nothing"
    )]
    /// Preserves the owned platform observation through this transition.
    pub(super) fn membership(&self) -> Option<std::sync::Arc<Membership>> {
        Some(std::sync::Arc::clone(&self.membership))
    }

    /// Windows has no polite phase: termination is immediate by construction.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn terminate_gently(&self) -> io::Result<()> {
        self.terminate_forcefully(LeaderObservation::Running)
    }

    /// Preserves the owned platform observation through this transition.
    pub(super) fn terminate_forcefully(&self, _leader: LeaderObservation) -> io::Result<()> {
        #[expect(unsafe_code, reason = "TerminateJobObject has no safe binding")]
        let terminated = unsafe { TerminateJobObject(self.job()?, TERMINATED_JOB_EXIT_CODE) };
        if terminated == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn active(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let size = u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
            .map_err(io::Error::other)?;
        #[expect(
            unsafe_code,
            reason = "QueryInformationJobObject observes the existing owned job boundary"
        )]
        let queried = unsafe {
            QueryInformationJobObject(
                self.job()?,
                JobObjectBasicAccountingInformation,
                std::ptr::addr_of_mut!(accounting).cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses)
    }

    /// Confirms zero active members after observing job events or the OS completion backstop.
    pub(super) fn settle(&self, leader: LeaderObservation) -> io::Result<()> {
        if self.job.is_none() {
            return Ok(());
        }
        self.terminate_forcefully(leader)?;
        let mut named = self
            .membership
            .queue
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        let deadline = Instant::now()
            .checked_add(super::REAPING_GRACE)
            .ok_or_else(|| io::Error::other("the job completion deadline is not representable"))?;
        while self.active()? != 0 {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the job still owns active processes",
                    )
                })?;
            let milliseconds = u32::try_from(left.as_millis()).map_err(io::Error::other)?;
            let milliseconds = if left.subsec_nanos() % 1_000_000 == 0 {
                milliseconds
            } else {
                milliseconds
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("the kernel wait quantum exceeds its width"))?
            };
            if let Some((message, detail)) = self.membership.receive(milliseconds)? {
                retain_member(&mut named, message, detail);
            }
        }
        Ok(())
    }

    fn job(&self) -> io::Result<HANDLE> {
        self.job
            .as_ref()
            .map(OwnedHandle::as_raw_handle)
            .ok_or_else(|| io::Error::other("the owned job was already closed"))
    }

    /// Closing the last handle kills whatever is still in the job.
    /// What the supervisor can say about the set it owns, for the note before an abort.
    #[expect(
        clippy::unused_self,
        reason = "the same signature as the unix supervisor, which reports the group it holds"
    )]
    /// Preserves the owned platform observation through this transition.
    pub(super) fn state(&self) -> String {
        "holding a job object".to_owned()
    }

    /// Preserves the owned platform observation through this transition.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn release(&mut self) -> io::Result<()> {
        if self.job.is_none() {
            return Ok(());
        }
        if self.active()? != 0 {
            self.terminate_forcefully(LeaderObservation::Running)?;
            self.settle(LeaderObservation::Running)?;
        }
        let released_job = self.job.take();
        drop(released_job);
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if let Err(source) = self.release() {
            super::group::terminal(&format!(
                "the owned job could not settle before release: {source}"
            ));
        }
    }
}

#[derive(Debug)]
pub(crate) struct ExitHandle(OwnedHandle);

impl ExitHandle {
    /// Cancels the raw child retained during a failed or unwinding preparation.
    pub(super) fn stop_owned(child: &mut Child) -> io::Result<()> {
        child.kill()
    }

    pub(super) fn of(child: &Child) -> io::Result<Self> {
        use std::os::windows::io::AsHandle as _;

        child.as_handle().try_clone_to_owned().map(Self)
    }

    pub(super) fn wait(self) -> io::Result<()> {
        #[expect(
            unsafe_code,
            reason = "WaitForSingleObject is the existing process event boundary"
        )]
        let observed = unsafe {
            WaitForSingleObject(
                self.0.as_raw_handle(),
                windows_sys::Win32::System::Threading::INFINITE,
            )
        };
        match observed {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            unexpected => Err(io::Error::other(format!(
                "the blocking process wait returned {unexpected}"
            ))),
        }
    }
}

fn resume_entry_point() -> io::Result<NtResumeProcess> {
    let module_name: Vec<u16> = "ntdll.dll"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    #[expect(unsafe_code, reason = "GetModuleHandleW has no safe binding")]
    let module = unsafe { GetModuleHandleW(module_name.as_ptr()) };
    if module.is_null() {
        return Err(unavailable("could not find ntdll.dll"));
    }
    #[expect(unsafe_code, reason = "GetProcAddress has no safe binding")]
    let address = unsafe { GetProcAddress(module, c"NtResumeProcess".as_ptr().cast()) };
    let Some(address) = address else {
        return Err(unavailable("could not find NtResumeProcess in ntdll.dll"));
    };
    #[expect(
        unsafe_code,
        reason = "the resolved symbol is the documented NtResumeProcess signature"
    )]
    let resume = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, NtResumeProcess>(address)
    };
    Ok(resume)
}

#[derive(Debug)]
struct ReaderState {
    stopped: Mutex<bool>,
    changed: std::sync::Condvar,
}

/// A retained stop subscription for a Windows anonymous output pipe.
#[derive(Debug)]
pub struct ReaderWait {
    state: std::sync::Arc<ReaderState>,
}

/// The single producer of an output reader's finite stop event.
#[derive(Debug)]
pub struct ReaderStop {
    state: std::sync::Arc<ReaderState>,
}

impl ReaderWait {
    /// Creates the stop subscription before its reader starts.
    ///
    /// # Errors
    /// Other supported platforms may refuse the required kernel subscription.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the POSIX counterpart creates a fallible kernel stop descriptor"
    )]
    pub fn channel() -> io::Result<(Self, ReaderStop)> {
        let state = std::sync::Arc::new(ReaderState {
            stopped: Mutex::new(false),
            changed: std::sync::Condvar::new(),
        });
        Ok((
            Self {
                state: std::sync::Arc::clone(&state),
            },
            ReaderStop { state },
        ))
    }

    /// Waits on the owned stop event until the typed anonymous-pipe readiness backstop.
    ///
    /// # Errors
    /// The retained stop state became poisoned.
    pub fn wait(&self, _reader: &io::PipeReader) -> io::Result<super::ReaderReady> {
        let stopped = self
            .state
            .stopped
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        let (stopped, timeout) = self
            .state
            .changed
            .wait_timeout_while(stopped, super::ANONYMOUS_PIPE_BACKSTOP, |stopped| !*stopped)
            .map_err(|source| io::Error::other(source.to_string()))?;
        if *stopped {
            Ok(super::ReaderReady::Stopped)
        } else if timeout.timed_out() {
            Ok(super::ReaderReady::AnonymousPipeBackstop)
        } else {
            Err(io::Error::other(
                "the pipe backstop ended without its deadline or stop event",
            ))
        }
    }
}

impl ReaderStop {
    /// Publishes the monotonic stop state before waking the reader.
    ///
    /// # Errors
    /// The retained stop state became poisoned.
    pub fn stop(&self) -> io::Result<()> {
        *self
            .state
            .stopped
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))? = true;
        self.state.changed.notify_all();
        Ok(())
    }
}

impl Drop for ReaderStop {
    fn drop(&mut self) {
        if let Err(source) = self.stop() {
            super::group::terminal(&format!(
                "the pipe owner could not publish its stop event: {source}"
            ));
        }
    }
}

#[derive(Debug)]
pub(crate) struct ForeignHandle {
    pub(super) identity: super::ProcessIdentity,
    handle: OwnedHandle,
}

impl ForeignHandle {
    pub(super) fn retain(pid: u32) -> io::Result<Option<Self>> {
        if pid == 0 {
            return Err(io::Error::other(
                "a retained process identity needs a positive PID",
            ));
        }
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        };
        #[expect(
            unsafe_code,
            reason = "OpenProcess retains the exact foreign kernel process object for identity, cancellation and completion"
        )]
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            let source = io::Error::last_os_error();
            if source.kind() == io::ErrorKind::InvalidInput {
                return Ok(None);
            }
            return Err(source);
        }
        #[expect(
            unsafe_code,
            reason = "the newly opened process object transfers exactly once into its safe handle owner"
        )]
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let born = creation_time(&handle)?;
        let identity = super::ProcessIdentity {
            pid,
            born,
            boot: "filetime".to_owned(),
        };
        Ok(Some(Self { identity, handle }))
    }

    pub(super) fn wait(&self, bound: Option<std::time::Duration>) -> io::Result<bool> {
        let milliseconds = match bound {
            None => windows_sys::Win32::System::Threading::INFINITE,
            Some(bound) => {
                let millis = bound.as_millis();
                let rounded = if bound.subsec_nanos() % 1_000_000 == 0 {
                    millis
                } else {
                    millis
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("the process wait exceeds its width"))?
                };
                let milliseconds = u32::try_from(rounded).map_err(io::Error::other)?;
                if milliseconds == windows_sys::Win32::System::Threading::INFINITE {
                    return Err(io::Error::other(
                        "a finite process wait cannot alias INFINITE",
                    ));
                }
                milliseconds
            }
        };
        #[expect(
            unsafe_code,
            reason = "WaitForSingleObject observes the retained foreign process kernel object rather than its reusable PID"
        )]
        let waited = unsafe { WaitForSingleObject(self.handle.as_raw_handle(), milliseconds) };
        match waited {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            unexpected => Err(io::Error::other(format!(
                "unexpected retained process wait: {unexpected}"
            ))),
        }
    }

    pub(super) fn stop(&self) -> io::Result<()> {
        if self.wait(Some(std::time::Duration::ZERO))? {
            return Ok(());
        }
        #[expect(
            unsafe_code,
            reason = "TerminateProcess acts on the retained kernel process object, never a recycled numeric PID"
        )]
        let stopped = unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(
                self.handle.as_raw_handle(),
                TERMINATED_JOB_EXIT_CODE,
            )
        };
        if stopped != 0 || self.wait(Some(std::time::Duration::ZERO))? {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

fn creation_time(handle: &OwnedHandle) -> io::Result<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    #[expect(
        unsafe_code,
        reason = "GetProcessTimes reads the retained kernel object's exact creation generation"
    )]
    let read = unsafe {
        windows_sys::Win32::System::Threading::GetProcessTimes(
            handle.as_raw_handle(),
            std::ptr::addr_of_mut!(created),
            std::ptr::addr_of_mut!(exited),
            std::ptr::addr_of_mut!(kernel),
            std::ptr::addr_of_mut!(user),
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}
