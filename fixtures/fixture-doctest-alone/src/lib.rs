// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Doctests an edition before 2024 compiles one binary each: one that returns, one that should panic, and ones rustdoc only compiles or skips.

/// The sum of `a` and `b`.
///
/// ```
/// assert_eq!(fixture_doctest_alone::add(2, 3), 5);
/// ```
///
/// ```ignore
/// this example is never compiled
/// ```
///
/// ```compile_fail
/// let sum: u8 = fixture_doctest_alone::add(2, 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// `a` divided by `b`, refusing a zero divisor with a panic of its own.
///
/// ```should_panic
/// fixture_doctest_alone::divide(1, 0);
/// ```
pub fn divide(a: i32, b: i32) -> i32 {
    if b == 0 {
        panic!("a zero divisor");
    }
    a.checked_div(b).unwrap_or_default()
}

/// The number after `n`, whose example rustdoc compiles and never runs.
///
/// ```no_run
/// assert_eq!(fixture_doctest_alone::next(1), 2);
/// ```
pub fn next(n: u32) -> u32 {
    n + 1
}
