// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run is called.

use jiff::Timestamp;
use rust_mutants::id::{RunId, RunIdError};

/// The name of the run that started at `now` in process `process`.
///
/// # Errors
/// Returns an error if the timestamp formatter ever stops producing the deliberately narrow run-id alphabet.
pub fn mint(now: Timestamp, process: u32) -> Result<RunId, RunIdError> {
    let stamp = now
        .strftime("%Y%m%dT%H%M%S%3fZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    RunId::try_from(format!("{stamp}-{process:06x}"))
}
