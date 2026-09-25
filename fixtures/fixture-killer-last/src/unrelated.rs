// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A compiled file no mutation of `double` depends on, which a test edits to make the outcome store miss.

/// A constant nothing else reads.
#[must_use]
pub const fn unrelated() -> u32 {
    7
}
