// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Depends on nothing a machine sets.

#[test]
fn twice_two_is_four() {
    assert_eq!(environment::double(2), 4);
}
