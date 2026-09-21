// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree that reads a library through a path climbing more than one level out of it.

/// Four times `n`, and one more.
#[must_use]
pub fn quadrupled_and_one(n: i32) -> i32 {
    fixture_climbs_dep_lib::quadrupled(n) + 1
}

#[cfg(test)]
mod tests {
    use super::quadrupled_and_one;

    #[test]
    fn quadrupling_two_and_adding_one_is_nine() {
        assert_eq!(quadrupled_and_one(2), 9);
    }
}
