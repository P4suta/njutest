// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose only test keeps what it makes in the directory cargo gives an integration test to write in.

/// The text a test keeps for `value`: the number after it.
#[must_use]
pub fn kept(value: u32) -> String {
    (value + 1).to_string()
}
