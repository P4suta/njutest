// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unused_qualifications)]

//! A crate whose root forbids a lint the guards turn off, so no guard could compile in it.

/// The larger of two numbers.
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

#[cfg(test)]
mod tests {
    #[test]
    fn max_picks_the_larger() {
        assert_eq!(super::max(1, 2), 2);
    }
}
