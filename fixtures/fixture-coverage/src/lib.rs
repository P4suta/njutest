// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conditions a branch proof can be stated about, and conditions it cannot.

/// A condition of primitives, which the compiler vouches for: `le-to-lt` here carries a proof.
pub fn clamp(value: i32, limit: i32) -> i32 {
    if value <= limit {
        return value;
    }
    limit
}

/// A condition whose operands are not primitives. The syntax says the same thing about it; the compiler refuses the witness, so no proof is stated.
pub fn earlier(a: Version, b: Version) -> bool {
    if a <= b {
        return true;
    }
    false
}

/// A version, ordered by a comparison the compiler does not vouch for.
#[derive(PartialEq, PartialOrd)]
pub struct Version(pub u32);

/// A condition that runs the program's code, which no proof is stated about.
pub fn short(items: &[i32]) -> bool {
    if items.len() <= 2 {
        return true;
    }
    false
}

/// A condition over text, whose comparison is the library's rather than the program's: the compiler vouches for it too.
pub fn named(name: &str) -> bool {
    if name <= "m" {
        return true;
    }
    false
}

/// A condition whose two comparisons are between different types, both of them the standard library's. One refused witness refuses the whole condition, so this is what says neither is refused.
pub fn labelled(label: String, rank: u8) -> bool {
    if label == "target" && rank <= 3 {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn clamp_returns_the_smaller() {
        assert_eq!(super::clamp(1, 2), 1);
        assert_eq!(super::clamp(3, 2), 2);
    }

    #[test]
    fn a_version_is_earlier_than_a_later_one() {
        assert!(super::earlier(super::Version(1), super::Version(2)));
        assert!(!super::earlier(super::Version(3), super::Version(2)));
    }

    #[test]
    fn a_name_before_m_is_named() {
        assert!(super::named("alpha"));
        assert!(!super::named("zulu"));
    }

    #[test]
    fn a_labelled_target_of_low_rank_is_labelled() {
        assert!(super::labelled("target".to_owned(), 1));
        assert!(!super::labelled("other".to_owned(), 1));
    }

    #[test]
    fn a_short_list_is_short() {
        assert!(super::short(&[1, 2]));
        assert!(!super::short(&[1, 2, 3]));
    }
}
