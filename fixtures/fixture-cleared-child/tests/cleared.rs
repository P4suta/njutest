// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starts the child with nothing of this process's environment and checks what it printed.

#[test]
fn the_child_prints_the_answer_whatever_environment_it_was_given() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_child"))
        .env_clear()
        .output()
        .expect("the child runs");
    assert_eq!(String::from_utf8(output.stdout).expect("text").trim(), "42");
}
