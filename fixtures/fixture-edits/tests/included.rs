// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads the data module as text, and never calls it.

#[test]
fn the_data_module_says_one() {
    assert!(edits::included::SOURCE.contains("    1\n"));
}
