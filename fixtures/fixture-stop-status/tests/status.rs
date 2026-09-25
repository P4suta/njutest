// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one test reaches the call that writes, and ends with the stop's status whenever a perturbation is active.

#[test]
fn a_count_is_kept() {
    let path = std::env::temp_dir().join("fixture-stop-status");
    fixture_stop_status::save(&path, 1).expect("the count is kept");
    assert_eq!(std::fs::read_to_string(&path).expect("the count reads"), "1");
}
