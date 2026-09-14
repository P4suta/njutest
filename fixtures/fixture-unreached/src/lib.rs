// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One function the tests call and one they do not, so a run has a mutation nothing reaches.

/// Twice `n`. A test calls it, so what a mutation of it does is something the tests answer for.
pub fn double(n: i32) -> i32 {
    n * 2
}

/// Half of `n`. Nothing calls it, so no measured test reaches a mutation of it.
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
