// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose returned type has a `Default` and no equality a probe may trust.

/// A reading, which a replacement can default and no probe can compare.
#[derive(Debug, Default, Clone)]
pub struct Reading {
    /// What was read.
    pub value: u32,
}

/// The reading this crate always takes, returned by name, which is what a probe asks about.
#[must_use]
pub fn reading() -> Reading {
    let taken = Reading { value: 3 };
    taken
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_reading_is_three() {
        assert_eq!(super::reading().value, 3);
    }
}
