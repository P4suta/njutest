// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Windows supervision: a Job Object per child.
//!
//! A Job Object with kill-on-close is created before the child starts, the
//! child is created suspended and assigned to it before it has run a single
//! instruction, and every process it later creates joins the job. Closing
//! the last handle kills whatever is still in it, so even a panicking
//! supervisor cannot leak the tree. Resuming the suspended child costs one
//! undocumented call, `NtResumeProcess`, because the supported route is a
//! system-wide thread snapshot per mutant.

#![allow(
    unsafe_code,
    reason = "Job Objects, process assignment, and NtResumeProcess have no safe binding"
)]

use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, ExitStatus};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

use super::{EXIT_CODE_UNAVAILABLE, RunnerError};

/// The mechanism this platform supervises with.
pub(super) const SUPERVISOR_KIND: &str = "job-object";

/// The status `TerminateJobObject` stamps on every process in the job. It
/// never reaches a caller.
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
    /// Creates and configures the job object before the child exists, so a
    /// machine that cannot create job objects is discovered while nothing is
    /// running.
    pub(super) fn new() -> Result<Self, RunnerError> {
        // SAFETY: plain FFI with null security attributes and no name.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(unavailable(
                "could not create the job object that owns the child process tree",
            ));
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0);
        // SAFETY: `info` is a valid, initialized structure of the stated size.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                size,
            )
        };
        if ok == 0 {
            let error =
                unavailable("could not configure the job object to kill its processes on close");
            // SAFETY: the handle is ours and unused after this.
            unsafe { CloseHandle(job) };
            return Err(error);
        }
        Ok(Self { job })
    }

    /// The child is created suspended, so that it can be assigned to the job
    /// before it has run an instruction: a process that has never executed
    /// cannot have forked.
    pub(super) fn configure(&self, command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    /// Assigns the suspended child to the job and resumes it. A failure
    /// leaves the child suspended and owned by nobody; the caller kills it.
    pub(super) fn adopt(&mut self, child: &Child) -> Result<(), RunnerError> {
        let process = child.as_raw_handle() as HANDLE;
        // SAFETY: both handles are valid for the duration of the call.
        if unsafe { AssignProcessToJobObject(self.job, process) } == 0 {
            return Err(unavailable(
                "could not assign the child process to the job object",
            ));
        }
        let resume = resume_entry_point()?;
        // SAFETY: `resume` is the NtResumeProcess entry point of ntdll, called
        // with a valid process handle.
        if unsafe { resume(process) } != 0 {
            return Err(RunnerError::SupervisionUnavailable {
                message: "NtResumeProcess refused to resume the suspended child".to_owned(),
                source: None,
            });
        }
        Ok(())
    }

    /// Windows has no polite phase: termination is immediate by construction.
    pub(super) fn terminate_gently(&self) {
        self.terminate_forcefully();
    }

    pub(super) fn terminate_forcefully(&self) {
        // SAFETY: the job handle is ours and valid until `release`.
        unsafe { TerminateJobObject(self.job, TERMINATED_JOB_EXIT_CODE) };
    }

    /// Closing the last handle kills whatever is still in the job.
    pub(super) fn release(&mut self) {
        if !self.job.is_null() {
            // SAFETY: the handle is ours and unused after this.
            unsafe { CloseHandle(self.job) };
            self.job = std::ptr::null_mut();
        }
    }
}

fn resume_entry_point() -> Result<NtResumeProcess, RunnerError> {
    let module_name: Vec<u16> = "ntdll.dll"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the name is a valid NUL-terminated UTF-16 string.
    let module = unsafe { GetModuleHandleW(module_name.as_ptr()) };
    if module.is_null() {
        return Err(unavailable("could not find ntdll.dll"));
    }
    // SAFETY: the name is a valid NUL-terminated ASCII string.
    let address = unsafe { GetProcAddress(module, c"NtResumeProcess".as_ptr().cast()) };
    let Some(address) = address else {
        return Err(unavailable("could not find NtResumeProcess in ntdll.dll"));
    };
    // SAFETY: NtResumeProcess has the signature `NTSTATUS(HANDLE)`.
    Ok(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, NtResumeProcess>(address)
    })
}

/// The child's status; a process the job terminated is reported by the
/// caller as unavailable, so only a real exit reaches here.
pub(super) fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(EXIT_CODE_UNAVAILABLE)
}
