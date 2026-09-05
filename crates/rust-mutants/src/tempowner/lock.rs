// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An exclusive advisory lock on one open file.
//!
//! The lock belongs to the open file, not to the process: two [`acquire`]
//! calls in one program contend with each other exactly as two processes
//! do, which is what makes concurrent opens in a single caller see each
//! other's directories as live.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// An exclusive advisory lock held on one open file. Dropping it releases
/// the lock; [`Lock::release`] does so explicitly and reports failures.
#[derive(Debug)]
pub struct Lock {
    file: Option<File>,
}

/// Opens `path`, creating it, and takes the exclusive lock without blocking.
///
/// A lock somebody else holds is not an error: it is the answer, `None`. The
/// distinction keeps "the owner is alive" and "the filesystem would not
/// answer" apart, because a sweep that read the second as the first would
/// delete a running workspace.
///
/// # Errors
///
/// Returns the open or lock failure.
pub fn acquire(path: &Path) -> io::Result<Option<Lock>> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    // Owner-only: a lock file another user could truncate is not a lock.
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
    /// Unlocks and closes the file. Idempotent.
    ///
    /// # Errors
    ///
    /// Returns the unlock or close failure.
    pub fn release(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.take() {
            // The unlock is explicit even though closing the descriptor drops
            // the lock: the close is what actually releases it, and an
            // unlock-then-close says so to the next reader of this function.
            sys::unlock(&file)?;
            drop(file);
        }
        Ok(())
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _released = self.release();
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io;

    use rustix::fs::{FlockOperation, flock};
    use rustix::io::Errno;

    /// The BSD `flock` on the open file description, which is what makes the
    /// lock disappear when the process dies however it died — the property
    /// the whole sweep rests on. `EWOULDBLOCK` and `EAGAIN` are the same errno
    /// on Linux and different ones on some other systems, so both read as
    /// "somebody else holds it".
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
#[expect(
    unsafe_code,
    reason = "LockFileEx and UnlockFileEx are the platform's advisory lock and have no safe binding"
)]
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
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is a valid open file for the lifetime of `file`,
        // and `overlapped` outlives the call.
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };
        if ok != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(i32::try_from(ERROR_LOCK_VIOLATION).unwrap_or(33)) {
            return Ok(false);
        }
        Err(error)
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        // SAFETY: as for `try_lock`.
        let ok = unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
