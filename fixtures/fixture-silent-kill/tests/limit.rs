// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Holds the limit to the one a child started with a cleared environment says.

#[test]
fn the_limit_is_the_one_a_cleared_child_says() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_child"))
        .env_clear()
        .output()
        .expect("the child runs");
    let said = String::from_utf8(output.stdout).expect("text");
    assert_eq!(said.trim(), fixture_silent_kill::limit().to_string());
}
