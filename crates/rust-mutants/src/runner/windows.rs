// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Windows supervision: a Job Object per child.

use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, ExitStatus};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Pipes::{PIPE_NOWAIT, SetNamedPipeHandleState};
use windows_sys::Win32::System::Threading::{CREATE_SUSPENDED, WaitForSingleObject};

use super::{LeaderObservation, ProcessExit, RunnerError, SupervisionBoundary};

/// The mechanism this platform supervises with.
pub(super) const SUPERVISOR_KIND: &str = "job-object";

/// A Job Object is an inescapable process-tree container.
pub(super) const SUPERVISION_BOUNDARY: SupervisionBoundary = SupervisionBoundary::ContainedTree;

/// Makes an anonymous pipe reader pollable so its owner can cancel and join it even when a descendant retained the write handle.
pub(super) fn configure_reader(reader: &io::PipeReader) -> io::Result<()> {
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
pub(super) struct Supervisor {
    job: HANDLE,
}

type NtResumeProcess = unsafe extern "system" fn(HANDLE) -> i32;

fn unavailable(message: &str) -> RunnerError {
    RunnerError::SupervisionUnavailable {
        message: message.to_owned(),
        source: Some(io::Error::last_os_error()),
    }
}

impl Supervisor {
    /// Creates and configures the job object before the child exists, so a machine that cannot create job objects is discovered while nothing is running.
    pub(super) fn new() -> Result<Self, RunnerError> {
        #[expect(unsafe_code, reason = "CreateJobObjectW has no safe binding")]
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(unavailable(
                "could not create the job object that owns the child process tree",
            ));
        }
        let supervisor = Self { job };
        #[expect(
            unsafe_code,
            reason = "the Windows structure is initialized through its documented zero state"
        )]
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size =
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|source| {
                RunnerError::SupervisionUnavailable {
                    message: "the Windows job information structure does not fit the platform API"
                        .to_owned(),
                    source: Some(io::Error::new(io::ErrorKind::InvalidData, source)),
                }
            })?;
        #[expect(unsafe_code, reason = "SetInformationJobObject has no safe binding")]
        let ok = unsafe {
            SetInformationJobObject(
                supervisor.job,
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
    pub(super) fn configure(&self, command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    /// Assigns the suspended child to the job and resumes it.
    /// Before assignment the caller owns the suspended child; after assignment this supervisor's Job Object owns it even when resuming fails.
    pub(super) fn adopt(&mut self, child: &Child) -> Result<(), RunnerError> {
        let process: HANDLE = child.as_raw_handle();
        #[expect(unsafe_code, reason = "AssignProcessToJobObject has no safe binding")]
        let assigned = unsafe { AssignProcessToJobObject(self.job, process) };
        if assigned == 0 {
            return Err(unavailable(
                "could not assign the child process to the job object",
            ));
        }
        let resume = resume_entry_point()?;
        #[expect(unsafe_code, reason = "calling NtResumeProcess is the FFI boundary")]
        let resumed = unsafe { resume(process) };
        if resumed != 0 {
            return Err(RunnerError::SupervisionUnavailable {
                message: "NtResumeProcess refused to resume the suspended child".to_owned(),
                source: None,
            });
        }
        Ok(())
    }

    /// Windows has no polite phase: termination is immediate by construction.
    pub(super) fn terminate_gently(&self) -> io::Result<()> {
        self.terminate_forcefully(LeaderObservation::Running)
    }

    pub(super) fn terminate_forcefully(&self, _leader: LeaderObservation) -> io::Result<()> {
        #[expect(unsafe_code, reason = "TerminateJobObject has no safe binding")]
        let terminated = unsafe { TerminateJobObject(self.job, TERMINATED_JOB_EXIT_CODE) };
        if terminated == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Closing the last handle kills whatever is still in the job.
    /// What the supervisor can say about the set it owns, for the note before an abort.
    pub(super) fn state(&self) -> String {
        "holding a job object".to_owned()
    }

    pub(super) fn release(&mut self) -> io::Result<()> {
        if self.job.is_null() {
            return Ok(());
        }
        #[expect(unsafe_code, reason = "CloseHandle has no safe binding")]
        let closed = unsafe { CloseHandle(self.job) };
        if closed == 0 {
            Err(io::Error::last_os_error())
        } else {
            self.job = std::ptr::null_mut();
            Ok(())
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if self.release().is_err() {
            std::process::abort();
        }
    }
}

/// Observes leader exit without reaping it.
/// The Job Object independently retains ownership of every contained descendant.
pub(super) fn exit_observed(child: &Child) -> io::Result<bool> {
    #[expect(
        unsafe_code,
        reason = "WaitForSingleObject is the Windows non-reaping process observation boundary"
    )]
    let observed = unsafe { WaitForSingleObject(child.as_raw_handle(), 0) };
    match observed {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        unexpected => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("WaitForSingleObject returned unexpected state {unexpected}"),
        )),
    }
}

fn resume_entry_point() -> Result<NtResumeProcess, RunnerError> {
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

/// The child's status; a process the job terminated is reported by the caller as unavailable, so only a real exit reaches here.
pub(super) fn process_exit(status: ExitStatus) -> ProcessExit {
    match status.code() {
        Some(code) => ProcessExit::Code(code),
        None => ProcessExit::Unknown,
    }
}
