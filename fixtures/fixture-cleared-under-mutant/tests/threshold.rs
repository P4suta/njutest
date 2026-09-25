// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asks the threshold about a small number, and asks a child with a cleared environment only when the threshold says to.

#[test]
fn a_small_number_is_checked_here() {
    let n = 7;
    if fixture_cleared_under_mutant::delegated(n) {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_child"))
            .arg(n.to_string())
            .env_clear()
            .output()
            .expect("the child runs");
        let said = String::from_utf8(output.stdout).expect("text");
        assert!(!said.trim().is_empty(), "the child answered");
    }
}
