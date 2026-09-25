// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asks the binary, in a process with nothing inherited, which side of zero one is on.

#[test]
fn one_is_positive() {
    let asked = std::process::Command::new(env!("CARGO_BIN_EXE_edits"))
        .arg("1")
        .env_clear()
        .output()
        .expect("the binary runs");
    assert_eq!(String::from_utf8_lossy(&asked.stdout), "positive\n");
}
