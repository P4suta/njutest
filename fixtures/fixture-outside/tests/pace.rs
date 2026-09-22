// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one test, which reaches both files.

#[test]
fn counting_to_four_sums_every_step_below_it() {
    assert_eq!(fixture_outside::count_to(4), 6);
    assert_eq!(fixture_outside::count_to(0), 0);
}
