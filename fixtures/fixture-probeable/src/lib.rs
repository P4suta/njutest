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

/// A value whose equality the standard library defines in full, so a guard may compare one.
#[derive(Debug, Clone, Default)]
pub struct Held {
    /// A name the tests only ever leave empty.
    pub name: String,
    /// A list the tests only ever leave empty.
    pub items: Vec<i32>,
    /// A number the tests only ever leave absent.
    pub maybe: Option<i32>,
}

/// Returns the name, which every test leaves empty, and empty is what the replacement writes.
#[must_use]
pub fn name_of(held: Held) -> String {
    held.name
}

/// Returns the list, which every test leaves empty, and empty is what the replacement writes.
#[must_use]
pub fn items_of(held: Held) -> Vec<i32> {
    held.items
}

/// Returns the number, which every test leaves absent, and absent is what the replacement writes.
#[must_use]
pub fn maybe_of(held: Held) -> Option<i32> {
    held.maybe
}

/// Returns a borrow of the name. The value is a `&String`, which has no `Default`, so the compiler refuses the probe however empty the name is.
#[must_use]
pub fn borrowed_name(held: &Held) -> &str {
    &held.name
}

/// The empty name, spelled as a name so the returned value is a `&str` rather than a borrow of a `String`.
pub const NO_NAME: &str = "";

/// Returns the name nothing has, which is already what the replacement would write.
#[must_use]
pub fn no_name() -> &'static str {
    NO_NAME
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
    fn a_held_name_comes_back_empty() {
        assert!(super::name_of(super::Held::default()).is_empty());
    }

    #[test]
    fn a_held_list_comes_back_empty() {
        assert!(super::items_of(super::Held::default()).is_empty());
    }

    #[test]
    fn a_held_number_comes_back_absent() {
        assert!(super::maybe_of(super::Held::default()).is_none());
    }

    #[test]
    fn a_borrowed_name_comes_back_empty() {
        assert!(super::borrowed_name(&super::Held::default()).is_empty());
    }

    #[test]
    fn nothing_has_a_name() {
        assert!(super::no_name().is_empty());
    }

    #[test]
    fn a_tag_comes_back_from_the_return() {
        assert_eq!(super::tagged().tag, "beta");
    }
}
