// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Checks the answer the build script decided.

#[test]
fn the_answer_is_forty_two() {
    assert_eq!(fixture_scripted::answer(), "42");
}
