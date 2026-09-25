// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One function, pinned by one target and run by another that asserts nothing about it.

/// Which side of zero `n` is on.
pub fn sign(n: i32) -> &'static str {
    if n > 0 { "positive" } else { "not positive" }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_library_has_a_test_of_its_own_that_reaches_nothing_of_it() {
        assert_eq!(1 + 1, 2);
    }
}
