// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two functions each with one example: one that runs on every target, and one that runs on every target but WebAssembly, which a sealed run therefore cannot hold.

/// Twice `n`, whose example runs natively and never on the sealed target.
///
/// ```ignore-wasm32
/// assert_eq!(fixture_doctest_host_only::twice(2), 4);
/// ```
pub fn twice(n: i32) -> i32 {
    n * 2
}

/// Thrice `n`, whose example runs everywhere.
///
/// ```
/// assert_eq!(fixture_doctest_host_only::thrice(2), 6);
/// ```
pub fn thrice(n: i32) -> i32 {
    n * 3
}
