// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three functions each with one example, merged into one binary that runs them in the order of their names: the first passes sealed, the second starts a thread, which the sealed host refuses, and the third passes sealed after it.

/// One more than `n`.
///
/// ```
/// assert_eq!(fixture_doctest_refused::after(1), 2);
/// ```
pub fn after(n: i32) -> i32 {
    n + 1
}

/// Twice `n`, worked out on a thread of its own, which a sealed run cannot start.
///
/// ```
/// let doubled = std::thread::spawn(|| fixture_doctest_refused::double(2)).join();
/// assert_eq!(doubled.expect("the thread returned"), 4);
/// ```
pub fn double(n: i32) -> i32 {
    n * 2
}

/// Half of `n`, whose example the merged binary reaches only after the one that stops it.
///
/// ```
/// assert_eq!(fixture_doctest_refused::half(4), 2);
/// ```
pub fn half(n: i32) -> i32 {
    n / 2
}
