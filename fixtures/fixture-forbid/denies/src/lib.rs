// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate that denies the same lint, which the guards' own attribute overrides.

/// The smaller of two numbers.
pub fn min(a: i32, b: i32) -> i32 {
    if a < b { a } else { b }
}

#[cfg(test)]
mod tests {
    #[test]
    fn min_picks_the_smaller() {
        assert_eq!(super::min(1, 2), 1);
    }
}
