// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads what the build script said.

#[test]
fn the_build_script_said_one() {
    assert_eq!(edits::built::built(), "one");
}
