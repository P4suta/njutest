// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A second target of the same tree that also fails, so a refusal has more than one thing to name.

/// The claim is wrong on purpose, and it is wrong in a second target so that a refusal
/// that stopped at the first would have something left to find.
#[test]
fn doubling_three_is_seven() {
    assert_eq!(fixture_verify_fails::double(3), 7);
}
