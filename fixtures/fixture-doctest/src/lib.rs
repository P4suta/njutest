// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose documentation runs, so a run has one target per library that carries no coverage.

/// Twice `n`.
///
/// ```
/// assert_eq!(fixture_doctest::double(2), 4);
/// ```
pub fn double(n: i32) -> i32 {
    n * 2
}

/// Half of `n`, documented without an example, so nothing about it runs.
pub fn half(n: i32) -> i32 {
    n / 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubling_two_is_four() {
        assert_eq!(super::double(2), 4);
    }
}
