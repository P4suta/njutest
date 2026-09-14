// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two conversions, one of which only a run that turned a feature on ever tests.

/// Metres from centimetres.
pub fn metres(centimetres: i32) -> i32 {
    centimetres / 100
}

/// Feet from inches.
pub fn feet(inches: i32) -> i32 {
    inches / 12
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_metre_is_a_hundred_centimetres() {
        assert_eq!(super::metres(250), 2);
    }
}

#[cfg(all(test, feature = "imperial"))]
mod imperial_tests {
    #[test]
    fn a_foot_is_twelve_inches() {
        assert_eq!(super::feet(30), 2);
    }
}
