// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library assembled out of two files nothing declares as modules, documented by a file that is not a program at all.
#![doc = include_str!("../README.md")]

include!("items.rs");

/// The numbers the table names.
pub const TABLE: [i32; 3] = include!("table.rs");

/// The sum of the table, doubled when `n` is over the threshold.
pub fn total(n: i32) -> i32 {
    let sum: i32 = TABLE.iter().sum();
    if over(n) { sum * 2 } else { sum }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_small_number_is_the_plain_sum() {
        assert_eq!(super::total(0), 8);
    }

    #[test]
    fn a_large_number_doubles_it() {
        assert_eq!(super::total(11), 16);
    }
}
