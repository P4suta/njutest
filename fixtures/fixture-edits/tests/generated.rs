// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads the value the generator wrote.

#[test]
fn the_generated_value_is_one() {
    assert_eq!(edits::generated::value(), 1);
}
