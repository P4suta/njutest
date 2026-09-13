// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library crate another member depends on.

mod util;

/// Clamps `n` into `[lo, hi]`.
pub fn clamp(n: i32, lo: i32, hi: i32) -> i32 {
    if n < lo {
        lo
    } else if n > hi {
        hi
    } else {
        n
    }
}

/// The sum of a slice, through a helper in a submodule.
pub fn total(xs: &[i32]) -> i32 {
    util::sum(xs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn clamp_bounds() {
        assert_eq!(super::clamp(5, 0, 3), 3);
        assert_eq!(super::clamp(-1, 0, 3), 0);
    }
}
