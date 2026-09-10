// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One command's answer, in the shape a spawned one has.
//!
//! A test that starts a process measures nothing about the code it is about: a
//! guard records what it reached in its own process, so every rule behind a
//! command whose only tests spawn the binary is reached by nobody. Driving the
//! same command through the library's own entry point in this process fixes
//! that, and the only thing in the way is the shape: a suite written against
//! [`std::process::Output`] would have to be rewritten line by line to read an
//! exit code out of something else.
//!
//! So the answer comes back in that shape. What is lost is the wiring in
//! `main.rs` between the process and the entry point, which is a handful of
//! lines and is what the remaining spawning tests are for.

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
