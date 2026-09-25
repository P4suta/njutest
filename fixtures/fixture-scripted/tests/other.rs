// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Checks the function the build script does not touch.

#[test]
fn other_is_one() {
    assert_eq!(fixture_scripted::other(), 1);
}
