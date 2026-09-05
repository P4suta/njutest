// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A workspace for the runner's baseline: two functions that no single test
//! reaches together, one test that libtest ignores, and an integration
//! target, so a baseline has something to route with and something to
//! refuse to call a pass.

/// Which side of zero `n` is on. Three regions, and one test reaches two of
/// them.
pub fn sign(n: i32) -> &'static str {
    if n > 0 {
        "positive"
    } else if n < 0 {
        "negative"
    } else {
        "zero"
    }
}

/// Twice `n`. Nothing in the library reaches this; only the integration
/// target does.
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn sign_names_both_sides_of_zero() {
        assert_eq!(super::sign(1), "positive");
        assert_eq!(super::sign(-1), "negative");
    }

    /// Ignored on purpose: a baseline must report it as skipped, and never
    /// as a pass it did not observe.
    #[test]
    #[ignore = "the baseline reports an ignored test as skipped, not as a pass"]
    fn zero_has_a_sign_of_its_own() {
        assert_eq!(super::sign(0), "zero");
    }
}
