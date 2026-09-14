// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run is called.

use jiff::Timestamp;

/// The name of the run that started at `now` in process `process`.
///
/// The instant is carried down to the millisecond, and not only because a
/// reader might want it: a run is stored under its name, so two runs sharing
/// one name are one directory, and the second is written over the first. Two
/// runs of one tree a moment apart is what a person does; two in one second is
/// what a script does. The process is in the name for the same reason, and
/// answers the other half of the question: two runs started together are two
/// machines' worth of work, not one run recorded twice.
#[must_use]
pub fn mint(now: Timestamp, process: u32) -> String {
    let stamp = now
        .strftime("%Y%m%dT%H%M%S%3fZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    format!("{stamp}-{process:06x}")
}
