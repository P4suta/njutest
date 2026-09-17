// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One command's answer, in the shape a spawned one has.

/// What one command said, as a spawned process would have said it.
#[must_use]
pub fn answered(code: u8, out: Vec<u8>, err: Vec<u8>) -> std::process::Output {
    std::process::Output {
        status: status(code),
        stdout: out,
        stderr: err,
    }
}

/// An exit status that answers `code` to [`std::process::ExitStatus::code`].
#[cfg(unix)]
fn status(code: u8) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    std::process::ExitStatus::from_raw(i32::from(code).wrapping_shl(8))
}

/// An exit status that answers `code` to [`std::process::ExitStatus::code`].
#[cfg(windows)]
fn status(code: u8) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt as _;
    std::process::ExitStatus::from_raw(u32::from(code))
}
