// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the binary says, which is the only way anything here reaches the library.

#[test]
fn the_binary_names_both_sides_of_zero() {
    assert_eq!(said("1"), "positive");
    assert_eq!(said("-1"), "not positive");
}

fn said(argument: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fixture-subprocess"))
        .arg(argument)
        .output()
        .expect("the binary runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
