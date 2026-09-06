// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that runs the condition of `clamp` and never the branch it gates.

#[test]
fn a_value_above_the_limit_is_the_limit() {
    assert_eq!(fixture_coverage::clamp(3, 2), 2);
}
