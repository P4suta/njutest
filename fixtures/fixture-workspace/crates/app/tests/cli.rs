// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs the binary cargo built, the way cargo's own test environment allows.

#[test]
fn sums_and_clamps() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fixture-app"))
        .args(["40", "50"])
        .output()
        .expect("runs");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "90");
    assert!(output.status.success());
}
