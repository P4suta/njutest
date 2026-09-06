// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The other member that compiles the shared file. Only its tests exercise `at_least`.

#[path = "../../shared/util.rs"]
pub mod util;

#[cfg(test)]
mod tests {
    #[test]
    fn the_floor_is_the_least_it_can_be() {
        assert_eq!(super::util::at_least(1, 3), 3);
        assert_eq!(super::util::at_least(4, 3), 4);
    }
}
