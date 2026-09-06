// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Four markers: one on a line of code, one above an item, one above a statement, and one that hides nothing.

/// The larger of two numbers.
#[must_use]
pub fn larger(a: i32, b: i32) -> i32 {
    if a > b { a } else { b } // rust-mutants: skip the tie goes either way
}

// rust-mutants: skip the version string is not behaviour
/// What this library calls itself.
#[must_use]
pub fn name() -> &'static str {
    "annotated"
}

/// `n` doubled, having said so.
#[must_use]
pub fn doubled(n: i32) -> i32 {
    // rust-mutants: skip the log line is not the answer
    let _said = format!("doubling {n}");
    n * 2
}

// rust-mutants: skip nothing starts here
// The comment above claims a place no rule targets, which is what an
// unmatched-skip finding is for.

#[cfg(test)]
mod tests {
    use super::{doubled, larger, name};

    #[test]
    fn the_larger_of_two_is_the_larger() {
        assert_eq!(larger(1, 2), 2);
        assert_eq!(larger(3, 2), 3);
    }

    #[test]
    fn doubling_is_addition_twice() {
        assert_eq!(doubled(3), 6);
    }

    #[test]
    fn the_name_is_the_name() {
        assert_eq!(name(), "annotated");
    }
}
