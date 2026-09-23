// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A function documented with an example.

/// Twice `n`.
///
/// ```
/// assert_eq!(edits::documented::double(2), 5);
/// ```
pub fn double(n: i32) -> i32 {
    n * 2
}
