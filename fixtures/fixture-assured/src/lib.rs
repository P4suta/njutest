// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A workspace whose tests notice every change a mutation makes.

/// Whether `n` is above zero.
#[must_use]
pub fn is_positive(n: i32) -> bool {
    n > 0
}

/// Twice `n`.
#[must_use]
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn is_positive_is_false_at_zero_and_below() {
        assert!(super::is_positive(1));
        assert!(!super::is_positive(0));
        assert!(!super::is_positive(-1));
    }
}
