// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Return replacements a probe can answer for, and one it cannot.

/// Returns a value a test can tell from the default, so a probe records the infection.
pub fn seven() -> i32 {
    7
}

/// Returns the default on one input and not on another, so which test infects it depends on the test.
pub fn double(n: i32) -> i32 {
    n * 2
}

/// Returns a value the probe cannot ask about: comparing it means running the program's code.
pub fn measured(items: &[i32]) -> usize {
    items.len()
}

/// Returns a float, which the compiler refuses to probe because `-0.0` equals `0.0` and is not what the default writes.
pub fn ratio() -> f64 {
    1.5
}

#[cfg(test)]
mod tests {
    #[test]
    fn seven_is_seven() {
        assert_eq!(super::seven(), 7);
    }

    #[test]
    fn doubling_zero_is_zero() {
        assert_eq!(super::double(0), 0);
    }

    #[test]
    fn doubling_four_is_eight() {
        assert_eq!(super::double(4), 8);
    }

    #[test]
    fn an_empty_list_measures_zero() {
        assert_eq!(super::measured(&[]), 0);
    }

    #[test]
    fn a_ratio_is_a_ratio() {
        assert!(super::ratio() > 1.0);
    }
}
