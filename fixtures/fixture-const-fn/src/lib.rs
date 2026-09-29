// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three uses of a `const fn`: one only a test calls, one a `const` evaluates, and a chain of two a `static` evaluates through its outer link.

/// Whether `n` is below ten, which only a test asks, while the program runs.
#[must_use]
pub const fn below_ten(n: u32) -> bool {
    n < 10
}

/// Twice `n`, which the compiler evaluates for [`DOUBLED`] before the program runs.
#[must_use]
pub const fn double(n: u32) -> u32 {
    n * 2
}

/// Twenty-one doubled, by the compiler.
pub const DOUBLED: u32 = double(21);

/// One more than `n`: the inner link of the chain [`CHAINED`] evaluates.
#[must_use]
pub const fn successor(n: u32) -> u32 {
    n + 1
}

/// Twice one more than `n`: the outer link, which only [`CHAINED`] calls.
#[must_use]
pub const fn twice_successor(n: u32) -> u32 {
    successor(n) * 2
}

/// Three through the chain, by the compiler.
pub static CHAINED: u32 = twice_successor(3);

#[cfg(test)]
mod tests {
    #[test]
    fn zero_is_below_ten_and_ten_is_not() {
        assert!(super::below_ten(0));
        assert!(!super::below_ten(10));
    }

    #[test]
    fn the_compiler_computed_what_the_program_holds() {
        assert_eq!(super::DOUBLED, 42);
        assert_eq!(super::CHAINED, 8);
    }
}
