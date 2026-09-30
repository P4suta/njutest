// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Constants observed only through their compiled values.

/// The answer every test expects.
pub const ANSWER: u32 = 42;

/// A flag the tests expect to be set.
pub const ENABLED: bool = true;

/// A value no test observes.
pub const UNUSED: u32 = 9;

/// The largest value this type holds.
pub const LIMIT: u8 = 255;

/// A division whose zero denominator the compiler refuses.
pub const QUOTIENT: u32 = 1 / 1;

/// A type holding an associated constant.
pub struct Settings;

impl Settings {
    /// The associated value the tests expect.
    pub const VALUE: u32 = 7;
}

#[cfg(test)]
mod tests {
    #[test]
    fn compiled_values_are_observed() {
        assert_eq!(super::ANSWER, 42);
        assert!(super::ENABLED);
        assert_eq!(super::LIMIT, 255);
        assert_eq!(super::QUOTIENT, 1);
        assert_eq!(super::Settings::VALUE, 7);
    }
}
