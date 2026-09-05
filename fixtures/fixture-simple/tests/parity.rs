// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An integration test target.

#[test]
fn even_numbers_are_even() {
    assert!(fixture_simple::is_even(2));
    assert!(!fixture_simple::is_even(3));
}
