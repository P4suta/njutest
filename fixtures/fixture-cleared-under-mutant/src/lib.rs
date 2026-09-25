// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A check that starts a child only when a threshold says so, and the child's own arithmetic.

/// Whether `n` is large enough to be checked by a child process.
#[must_use]
pub fn delegated(n: u32) -> bool {
    n > 100
}

/// What the child prints for `n`.
#[must_use]
pub fn doubled(n: u32) -> u32 {
    n * 2
}
