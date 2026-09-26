// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library built on the other, whose returned type is shaped the same way.

/// A running count, which a replacement can default and no probe can compare.
#[derive(Debug, Default, Clone)]
pub struct Tally {
    /// How many.
    pub count: u32,
}

/// `n` added to the upstream reading, returned by name, which is what a probe asks about.
#[must_use]
pub fn tally(n: u32) -> Tally {
    let counted = Tally {
        count: n + upstream::reading().value,
    };
    counted
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tally_adds_the_reading() {
        assert_eq!(super::tally(1).count, 4);
    }
}
