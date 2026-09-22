// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The loop, in a file the mutation that stops it ending is not in.

/// Adds `step` to a running total until it reaches `n`.
#[must_use]
pub fn walk(n: u32, step: u32) -> u32 {
    let mut at = 0;
    let mut total = 0;
    while at < n {
        total += at;
        at += step;
    }
    total
}
