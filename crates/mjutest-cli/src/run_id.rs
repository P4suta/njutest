// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run is called.
//!
//! The name is the run's directory under `reports/runs`, so it has to sort
//! the way the runs happened and it has to be unique. A UTC timestamp gives
//! the first; the process id gives the second, because two runs in the same
//! second are two processes and one process is one run.
//!
//! No randomness: an identity a test cannot predict is an identity a golden
//! cannot hold, and the moment and the process are both arguments.

use jiff::Timestamp;

/// The name of the run that started at `now` in process `process`.
///
/// `20260905T081500Z-0004d2`: sortable, unique per process-second, and
/// safe as a path component on every platform.
#[must_use]
pub fn mint(now: Timestamp, process: u32) -> String {
    let stamp = now
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    format!("{stamp}-{process:06x}")
}
