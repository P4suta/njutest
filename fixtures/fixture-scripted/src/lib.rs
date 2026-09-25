// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One function whose answer a build script decides, and one it does not touch.

/// What the data file said when the build script read it.
#[must_use]
pub fn answer() -> &'static str {
    env!("FIXTURE_ANSWER")
}

/// One.
#[must_use]
pub fn other() -> u32 {
    1
}
