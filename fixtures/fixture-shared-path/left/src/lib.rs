// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One of the two members that compile the shared file. Only its tests exercise `within`.

#[path = "../../shared/util.rs"]
pub mod util;

#[cfg(test)]
mod tests {
    #[test]
    fn the_bound_is_inclusive() {
        assert!(super::util::within(3, 3));
        assert!(!super::util::within(4, 3));
    }
}
