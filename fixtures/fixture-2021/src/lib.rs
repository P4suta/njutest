// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An edition 2021 crate that names the standard library the older way.

extern crate alloc;

use alloc::vec::Vec;

/// The sum of the numbers, spelled with a fold a mutant can flip.
pub fn total(numbers: &[i32]) -> i32 {
    let mut sum = 0;
    for one in numbers {
        sum += *one;
    }
    sum
}

/// The numbers above the bound, in order.
pub fn above(numbers: &[i32], bound: i32) -> Vec<i32> {
    let mut kept = Vec::new();
    for one in numbers {
        if *one > bound {
            kept.push(*one);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_total_is_the_sum() {
        assert_eq!(super::total(&[1, 2, 3]), 6);
    }

    #[test]
    fn above_keeps_what_is_greater() {
        assert_eq!(super::above(&[1, 2, 3], 2), vec![3]);
    }
}
