// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads the line `late` calls `here` on.

#[test]
fn late_calls_here_on_line_nineteen() {
    assert_eq!(edits::located::late(), 19);
}
