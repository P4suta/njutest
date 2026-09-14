// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An integration target that pins doubling at a value nothing else returns.

#[test]
fn doubling_four_is_eight() {
    assert_eq!(fixture_assured::double(4), 8);
    assert_eq!(fixture_assured::double(-3), -6);
}
