// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose every test is ignored, so its target runs nothing and decides nothing.

/// Twice `n`.
#[must_use]
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    /// Ignored on purpose: a target that runs no test is not a target that failed.
    #[test]
    #[ignore = "the point of this fixture is a target that runs nothing"]
    fn doubling_two_is_four() {
        assert_eq!(double(2), 4);
    }

    /// Ignored for the same reason, so that the count is more than one.
    #[test]
    #[ignore = "the point of this fixture is a target that runs nothing"]
    fn doubling_three_is_six() {
        assert_eq!(double(3), 6);
    }
}
