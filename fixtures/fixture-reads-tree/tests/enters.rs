// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calls the one function a test calls.

#[test]
fn loud_doubles() {
    assert_eq!(fixture_reads_tree::loud(2), 4);
}
