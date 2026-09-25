// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The test that notices a change to `double`.

#[test]
fn doubling_three_is_six() {
    assert_eq!(fixture_killer_last::double(3), 6);
}
