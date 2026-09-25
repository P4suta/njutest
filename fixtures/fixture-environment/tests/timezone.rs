// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assumes the zone is not the one with a half-hour daylight saving shift.

#[test]
fn the_zone_is_not_lord_howe() {
    assert!(!environment::zone().contains("Lord_Howe"));
}
