// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Each function's own test, in a file no mutation is in.

#[test]
fn two_and_three_make_five() {
    assert_eq!(fixture_integration_bodies::total(2, 3), 5);
}

#[test]
fn ten_is_over_and_nine_is_not() {
    assert!(fixture_integration_bodies::over(10));
    assert!(!fixture_integration_bodies::over(9));
}
