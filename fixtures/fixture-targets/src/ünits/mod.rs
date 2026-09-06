// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A module in a directory whose name is not ASCII, which a report has to name and a snapshot has to copy.

/// How many of the thing there are in one unit.
pub const PER_UNIT: u32 = 4;

/// Whether `n` fills at least one unit.
#[must_use]
pub fn filled(n: u32) -> bool {
    n >= PER_UNIT
}
