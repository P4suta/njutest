// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three functions: one the tests and the documentation both exercise, one only the documentation does, and one neither does.

/// Twice `n`.
///
/// ```
/// assert_eq!(fixture_doctest::double(2), 4);
/// ```
pub fn double(n: i32) -> i32 {
    n * 2
}

/// Half of `n`. Only this example exercises it, so only this example can notice a mutation of it.
///
/// ```
/// assert_eq!(fixture_doctest::half(4), 2);
/// ```
pub fn half(n: i32) -> i32 {
    n / 2
}

/// A third of `n`, documented without an example, so nothing runs it.
pub fn third(n: i32) -> i32 {
    n / 3
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubling_two_is_four() {
        assert_eq!(super::double(2), 4);
    }
}
