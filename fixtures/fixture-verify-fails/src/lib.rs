// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose own test does not pass, so nothing a mutation of it does means anything.

/// Twice `n`.
#[must_use]
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    /// The claim is wrong on purpose: four is not five, so this target fails on the tree as
    /// committed and every outcome under a mutation would be the same failure.
    #[test]
    fn doubling_two_is_five() {
        assert_eq!(double(2), 5);
    }
}
