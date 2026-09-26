// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that writes under the home directory and reads back what it wrote.

#[test]
fn a_setting_kept_is_the_setting_recalled() {
    fixture_home::remember("kept").expect("the setting is written");
    assert_eq!(fixture_home::recall().expect("the setting is read"), "kept");
}
