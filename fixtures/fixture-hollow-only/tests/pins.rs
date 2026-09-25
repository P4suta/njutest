// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target asked after the smoke test, which pins every answer the library gives.

#[test]
fn sign_says_which_side_of_zero_a_number_is_on() {
    assert_eq!(fixture_hollow_only::sign(1), "positive");
    assert_eq!(fixture_hollow_only::sign(0), "not positive");
    assert_eq!(fixture_hollow_only::sign(-1), "not positive");
}
