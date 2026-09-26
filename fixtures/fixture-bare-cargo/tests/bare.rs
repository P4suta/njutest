// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that runs `cargo` by its bare name, from the directory the test runs in.

#[test]
fn a_bare_cargo_answers_where_the_tests_run() {
    let answered = std::process::Command::new("cargo")
        .arg("-V")
        .output()
        .expect("cargo starts");
    assert!(
        answered.status.success(),
        "cargo -V: {:?}",
        answered.stderr
    );
    assert_eq!(fixture_bare_cargo::double(3), 6);
}
