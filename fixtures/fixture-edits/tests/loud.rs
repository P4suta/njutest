// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads how loud the library was built.

#[test]
fn the_library_is_quiet() {
    assert_eq!(edits::loud::volume(), 1);
}
