// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An edition 2021 crate with a module whose directory is not spelled in ASCII.

#[path = "ünits/mod.rs"]
pub mod ünits;

/// The number of whole units in `n`.
#[must_use]
pub fn whole(n: u32) -> u32 {
    n / ünits::PER_UNIT
}

#[cfg(test)]
mod tests {
    use super::whole;

    #[test]
    fn seven_holds_one_whole_unit() {
        assert_eq!(whole(7), 1);
        assert_eq!(whole(3), 0);
    }
}
