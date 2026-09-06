// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two functions: one a mutation stops returning from, and one a mutation only a slow test could be undecided about.

/// The sum of every step below `n`.
#[must_use]
pub fn count_to(n: u32) -> u32 {
    let mut step = 0;
    let mut total = 0;
    while step < n {
        total += step;
        step += 1;
    }
    total
}

/// `n` where it is positive, and zero everywhere else.
///
/// The bound reads `>` where `>=` would do, because zero clamps to zero
/// either way: a mutation of it survives, which is what a run needs in order
/// to be undecided about one rather than sure of it.
#[must_use]
pub fn clamp_positive(n: i64) -> i64 {
    if n > 0 { n } else { 0 }
}
