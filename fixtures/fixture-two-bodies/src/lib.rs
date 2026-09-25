// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two functions no test of the other reaches, so an edit inside one body is one no execution of the other entered.

/// The sum of two counts.
pub fn total(left: u32, right: u32) -> u32 {
    left + right
}

/// Whether a count is over the limit.
pub fn over(count: u32) -> bool {
    count > 9
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_and_three_make_five() {
        assert_eq!(super::total(2, 3), 5);
    }

    #[test]
    fn ten_is_over_and_nine_is_not() {
        assert!(super::over(10));
        assert!(!super::over(9));
    }
}
