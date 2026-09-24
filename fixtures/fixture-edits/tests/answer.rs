// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads the answer the proc macro gave.

#[test]
fn the_answer_is_forty_two() {
    assert_eq!(edits::answered::answer(), 42);
}
