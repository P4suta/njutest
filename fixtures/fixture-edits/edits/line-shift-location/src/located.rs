// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Code whose line numbers are part of what it does.

/// The line it was called from.
#[track_caller]
pub fn here() -> u32 {
    std::panic::Location::caller().line()
}

/// A function nothing in the tests calls.
pub fn early() -> u32 {
    let one = 1;
    one
}

/// The line `here` is called on.
pub fn late() -> u32 {
    here()
}
