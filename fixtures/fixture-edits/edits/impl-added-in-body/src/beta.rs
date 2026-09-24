// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A type a test drops, and a function nothing in the tests calls.

/// Something dropped.
pub struct Beta;

/// A function nothing in the tests calls.
pub fn alpha() {
    #[expect(
        non_local_definitions,
        reason = "an impl written inside a body is the edit this class is"
    )]
    impl Drop for Beta {
        fn drop(&mut self) {
            panic!("a Beta was dropped")
        }
    }
}
