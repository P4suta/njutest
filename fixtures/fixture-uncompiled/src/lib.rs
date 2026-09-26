// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library with one tested function and a module no build of it compiles, as a module gated to another platform is on this one.

#[cfg(any())]
mod elsewhere;

/// The larger of two numbers.
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_larger_is_returned_whichever_comes_first() {
        assert_eq!(super::max(2, 3), 3);
        assert_eq!(super::max(3, 2), 3);
    }
}
