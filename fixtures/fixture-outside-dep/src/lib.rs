// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree that reads a library from beside itself, which a copy of the tree alone does not hold.

/// Three times `n`, and one more.
#[must_use]
pub fn tripled_and_one(n: i32) -> i32 {
    fixture_outside_dep_lib::tripled(n) + 1
}

#[cfg(test)]
mod tests {
    use super::tripled_and_one;

    #[test]
    fn tripling_two_and_adding_one_is_seven() {
        assert_eq!(tripled_and_one(2), 7);
    }
}
