// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two functions no test of the other reaches, each tested from an integration test rather than beside it.

/// The sum of two counts.
pub fn total(left: u32, right: u32) -> u32 {
    left + right
}

/// Whether a count is over the limit.
pub fn over(count: u32) -> bool {
    count > 9
}
