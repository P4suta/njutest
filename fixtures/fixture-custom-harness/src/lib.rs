// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose only test target says what it found by exiting.

/// Whether `n` is above the line.
#[must_use]
pub fn above(n: i32) -> bool {
    n > 10
}
