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
    fn a_short_list_is_short() {
        assert!(super::short(&[1, 2]));
        assert!(!super::short(&[1, 2, 3]));
    }
}
