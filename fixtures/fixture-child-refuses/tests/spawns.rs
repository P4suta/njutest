// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that runs this binary again as a child, handing on its environment through the library.

#[test]
fn the_child_given_this_environment_succeeds() {
    if std::env::var_os("FIXTURE_CHILD").is_some() {
        let _ = fixture_child_refuses::handed_on(String::new());
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().expect("this binary"));
    child
        .args(["--exact", "the_child_given_this_environment_succeeds"])
        .env("FIXTURE_CHILD", "1");
    if let Ok(catalog) = std::env::var("RUST_MUTANTS_CATALOG") {
        child.env(
            "RUST_MUTANTS_CATALOG",
            fixture_child_refuses::handed_on(catalog),
        );
    }
    let ran = child.output().expect("the child runs");
    assert!(
        ran.status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
}
