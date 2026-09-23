// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assumes output has room for forty columns.

#[test]
fn there_is_room_for_forty_columns() {
    assert!(environment::width() >= 40);
}
