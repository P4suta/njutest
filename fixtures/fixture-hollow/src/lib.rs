// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two functions and two targets, where one target runs what it is given and asserts nothing about it.

/// Which side of zero `n` is on. The library's own test pins both answers.
pub fn sign(n: i32) -> &'static str {
    if n > 0 {
        "positive"
    } else {
        "not positive"
    }
}

/// Twice `n`. Only the integration target reaches this, and it checks nothing.
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn sign_says_which_side_of_zero_a_number_is_on() {
        assert_eq!(super::sign(1), "positive");
        assert_eq!(super::sign(-1), "not positive");
    }
}
