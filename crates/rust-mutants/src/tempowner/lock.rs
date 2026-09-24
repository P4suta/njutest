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

/// What waiting for a lock came to.
#[derive(Debug)]
pub(super) enum Waited {
    /// The lock, on the file `path` still names.
    Taken(Lock),
    /// The file was removed while this waited, so a lock on it would be on a name nobody can find.
    Removed,
    /// Somebody kept it for longer than the wait.
    Kept,
}

/// Opens `path`, creating it, and waits up to `within` for the exclusive lock.
///
/// # Errors
/// Returns the open, lock, or inspection failure.
pub(super) fn wait(path: &Path, within: std::time::Duration) -> io::Result<Waited> {
    let started = std::time::Instant::now();
    loop {
        if let Some(lock) = acquire(path)? {
            let named = match lock.file.as_ref() {
                Some(file) => still_named(file, path)?,
                None => false,
            };
            return Ok(if named {
                Waited::Taken(lock)
            } else {
                Waited::Removed
            });
        }
        if started.elapsed() >= within {
            return Ok(Waited::Kept);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Whether the file `held` is still the one `path` names.
#[cfg(unix)]
fn still_named(held: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let held = held.metadata()?;
    match std::fs::metadata(path) {
        Ok(named) => Ok(named.dev() == held.dev() && named.ino() == held.ino()),
        Err(missing) if missing.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source),
    }
}

/// Whether the file `held` is still the one `path` names; Windows refuses to remove a file somebody has open, so it always is.
#[cfg(not(unix))]
fn still_named(_held: &File, _path: &Path) -> io::Result<bool> {
    Ok(true)
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
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

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
