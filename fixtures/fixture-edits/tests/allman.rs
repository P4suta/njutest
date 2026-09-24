// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Names what `make` returns without calling it.

#[test]
fn make_returns_the_unit_type() {
    assert_eq!(edits::allman::named(edits::allman::make), "unit");
}
