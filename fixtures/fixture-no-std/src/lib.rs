// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `#![no_std]` library whose tests run on the host, so the runtime module borrows `std` under a name of its own and the crate is measured like any other.

#![no_std]

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

/// Whether `a` is at least `b`.
pub fn at_least(a: u32, b: u32) -> bool {
    a >= b
}

#[cfg(test)]
mod tests {
    #[test]
    fn adding_is_adding() {
        assert_eq!(super::add(2, 3), 5);
    }

    #[test]
    fn at_least_is_true_at_the_boundary_and_false_below_it() {
        assert!(super::at_least(2, 2));
        assert!(!super::at_least(1, 2));
    }
}
