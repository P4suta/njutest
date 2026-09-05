// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run is called.

use jiff::Timestamp;

/// The name of the run that started at `now` in process `process`.
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
