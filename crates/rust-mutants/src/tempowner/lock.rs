// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An exclusive advisory lock on one open file.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// An exclusive advisory lock held on one open file.
/// Dropping it releases the lock; [`Lock::release`] does so explicitly and reports failures.
#[derive(Debug)]
pub struct Lock {
    file: Option<File>,
}

/// Opens `path`, creating it, and takes the exclusive lock without blocking.
///
/// # Errors
/// Returns the open or lock failure.
pub fn acquire(path: &Path) -> io::Result<Option<Lock>> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    if sys::try_lock(&file)? {
        Ok(Some(Lock { file: Some(file) }))
    } else {
        Ok(None)
    }
}

impl Lock {
    /// Unlocks and closes the file.
    /// Idempotent.
    ///
    /// # Errors
    /// Returns the unlock or close failure.
    pub fn release(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.take() {
            sys::unlock(&file)?;
            drop(file);
        }
        Ok(())
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Err(release) = self.release() {
            drop(release);
        }
    }
}

/// What the operating system says of one process now: when it started, that no such process runs, or nothing it could read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Start {
    /// It runs, and started when this says, as the operating system spells it.
    Running(String),
    /// No process of that pid runs.
    Absent,
    /// The operating system would not say.
    Unread,
}

pub(super) use sys::{boot, start_of};

#[cfg(target_os = "linux")]
mod started {
    use njutest_process::Asked;

    use super::Start;

    /// The start of `pid` in clock ticks since boot, as `/proc` says, which is absent once the process has been reaped.
    pub(super) fn start_of(pid: u32) -> Start {
        match njutest_process::procfs::parsed(pid) {
            Ok(Asked::Answered(stat)) => Start::Running(stat.born.to_string()),
            Ok(Asked::Gone) => Start::Absent,
            Err(_unreadable) => Start::Unread,
        }
    }

    /// The kernel's id of this boot.
    pub(super) fn boot() -> Option<String> {
        match std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
            Ok(id) => Some(id.trim().to_owned()),
            Err(_unreadable) => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::Start;

        #[test]
        fn a_reaped_holder_is_absent_rather_than_unread_and_this_process_runs() {
            let mut ended =
                njutest_process::GroupChild::start(&mut std::process::Command::new("true"))
                    .expect("true starts");
            let pid = ended.id().expect("the unreaped leader");
            ended.wait().expect("true is reaped");
            assert_eq!(
                super::start_of(pid),
                Start::Absent,
                "a holder that has been reaped no longer runs, which is no unread start"
            );
            let own = super::start_of(std::process::id());
            assert!(matches!(own, Start::Running(_)), "{own:?}");
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod started {
    use std::process::Command;

    use super::Start;

    /// The start of `pid` as `ps` spells it in UTC, to the second.
    pub(super) fn start_of(pid: u32) -> Start {
        let output = match Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", "UTC0")
            .output()
        {
            Ok(output) => output,
            Err(_no_ps) => return Start::Unread,
        };
        let said = match String::from_utf8(output.stdout) {
            Ok(said) => said.trim().to_owned(),
            Err(_not_text) => return Start::Unread,
        };
        match (output.status.success(), said.is_empty()) {
            (true, false) => Start::Running(said),
            (false, true) => Start::Absent,
            (true, true) | (false, false) => Start::Unread,
        }
    }

    /// When the machine booted, as `sysctl` spells its seconds.
    pub(super) fn boot() -> Option<String> {
        let output = match Command::new("sysctl")
            .args(["-n", "kern.boottime"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(_) | Err(_) => return None,
        };
        let said = match String::from_utf8(output.stdout) {
            Ok(said) => said,
            Err(_not_text) => return None,
        };
        said.split(',')
            .next()
            .filter(|seconds| seconds.contains("sec"))
            .map(|seconds| seconds.trim().to_owned())
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io;

    use rustix::fs::{FlockOperation, flock};
    use rustix::io::Errno;

    /// The BSD `flock` on the open file description, which is what makes the lock disappear when the process dies however it died — the property the whole sweep rests on.
    /// `EWOULDBLOCK` and `EAGAIN` are the same errno on Linux and different ones on some other systems, so both read as "somebody else holds it".
    pub(super) fn try_lock(file: &File) -> io::Result<bool> {
        match flock(file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(true),
            Err(errno) if errno == Errno::WOULDBLOCK || errno == Errno::AGAIN => Ok(false),
            Err(errno) => Err(errno.into()),
        }
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        flock(file, FlockOperation::Unlock).map_err(io::Error::from)
    }

    pub(in super::super) fn start_of(pid: u32) -> super::Start {
        super::started::start_of(pid)
    }

    pub(in super::super) fn boot() -> Option<String> {
        super::started::boot()
    }
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, ERROR_LOCK_VIOLATION, FILETIME, HANDLE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    };

    use super::Start;

    /// A handle to a process opened to be asked about, closed when it goes.
    struct Opened(HANDLE);

    impl Drop for Opened {
        fn drop(&mut self) {
            #[expect(unsafe_code, reason = "CloseHandle has no safe binding")]
            let closed = unsafe { CloseHandle(self.0) };
            if closed == 0 {
                std::process::abort();
            }
        }
    }

    /// The creation time of `pid`, in the 100-nanosecond intervals since 1601 Windows counts it in, where it still runs: a time no other process of any boot shares.
    pub(in super::super) fn start_of(pid: u32) -> Start {
        #[expect(unsafe_code, reason = "OpenProcess has no safe binding")]
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return match io::Error::last_os_error().raw_os_error().map(u32::try_from) {
                Some(Ok(code)) if code == ERROR_INVALID_PARAMETER => Start::Absent,
                Some(Ok(_) | Err(_)) | None => Start::Unread,
            };
        }
        let opened = Opened(handle);
        #[expect(unsafe_code, reason = "WaitForSingleObject has no safe binding")]
        let waited = unsafe { WaitForSingleObject(opened.0, 0) };
        if waited == WAIT_OBJECT_0 {
            return Start::Absent;
        }
        if waited != WAIT_TIMEOUT {
            return Start::Unread;
        }
        let empty = || FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut creation, mut exit, mut kernel, mut user) = (empty(), empty(), empty(), empty());
        #[expect(unsafe_code, reason = "GetProcessTimes has no safe binding")]
        let read = unsafe {
            GetProcessTimes(
                opened.0,
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
        };
        if read == 0 {
            return Start::Unread;
        }
        let created =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        Start::Running(created.to_string())
    }

    /// Nothing: the creation time [`start_of`] reads already tells one boot's process from another's.
    pub(in super::super) const fn boot() -> Option<String> {
        None
    }

    pub(super) fn try_lock(file: &File) -> io::Result<bool> {
        #[expect(
            unsafe_code,
            reason = "OVERLAPPED is initialized through its documented zero state"
        )]
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        #[expect(unsafe_code, reason = "LockFileEx has no safe binding")]
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                std::ptr::addr_of_mut!(overlapped),
            )
        };
        if ok != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        let Some(raw) = error.raw_os_error() else {
            return Err(error);
        };
        let Ok(unsigned) = u32::try_from(raw) else {
            return Err(error);
        };
        if unsigned == ERROR_LOCK_VIOLATION {
            return Ok(false);
        }
        Err(error)
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        #[expect(
            unsafe_code,
            reason = "OVERLAPPED is initialized through its documented zero state"
        )]
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        #[expect(unsafe_code, reason = "UnlockFileEx has no safe binding")]
        let ok = unsafe {
            UnlockFileEx(
                file.as_raw_handle(),
                0,
                1,
                0,
                std::ptr::addr_of_mut!(overlapped),
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
