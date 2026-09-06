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

/// A value whose equality answers about one field and whose tests read another.
#[derive(Debug, Clone)]
pub struct Tagged {
    /// The field equality answers about.
    pub number: i32,
    /// The field equality ignores and a test reads.
    pub tag: &'static str,
}

impl Default for Tagged {
    fn default() -> Self {
        Self {
            number: 0,
            tag: "",
        }
    }
}

impl PartialEq for Tagged {
    fn eq(&self, other: &Self) -> bool {
        self.number == other.number
    }
}

/// Returns a value equal to the default and not the default, so a probe that trusts `==` says a test saw nothing when it saw the tag.
pub fn tagged() -> Tagged {
    Tagged {
        number: 0,
        tag: "beta",
    }
}

/// What `retries` answers, spelled as a name rather than as the literal the default writes.
pub const NO_RETRIES: i32 = 0;

/// How many retries there are. The value already is what the replacement would write, so no test can have noticed it and the probe says so.
pub fn retries() -> i32 {
    NO_RETRIES
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

    #[test]
    fn there_are_no_retries() {
        assert_eq!(super::retries(), 0);
    }

    #[test]
    fn a_tag_comes_back_from_the_return() {
        assert_eq!(super::tagged().tag, "beta");
    }
}
