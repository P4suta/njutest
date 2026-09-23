// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A literal that decides whether a loop somewhere else ends.

pub mod spin;

/// The sum of every step below `n`, walked one at a time.
#[must_use]
pub fn count_to(n: u32) -> u32 {
    let step = 1;
    spin::walk(n, step)
}
