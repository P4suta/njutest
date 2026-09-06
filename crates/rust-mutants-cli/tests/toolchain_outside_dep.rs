// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree that reads code from beside itself: refused by name, and measured when somebody says it may.

#![expect(
    clippy::expect_used,
    reason = "the helper that starts the engine is not itself a test"
)]

use std::process::Output;

use mjutest_devkit::fixture::Fixture;

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.args(args);
    command.output().expect("rust-mutants runs")
}

#[test]
fn a_path_dependency_outside_the_root_is_named_before_any_build() {
    let fixture = Fixture::copy_with_siblings("fixture-outside-dep", &["fixture-outside-dep-lib"]);
    let output = run(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a tree a copy of which cannot build is refused rather than measured"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("RM1017"), "{said}");
    assert!(
        said.contains("fixture-outside-dep-lib"),
        "the refusal names the dependency: {said}"
    );
    assert!(
        said.contains("--allow-outside"),
        "and the flag that allows it: {said}"
    );
    assert!(
        !said.contains("could not compile") && !said.contains("no such file"),
        "and says it before cargo has anything to say about a manifest that is not there: {said}"
    );
}

#[test]
fn an_allowed_sibling_is_copied_beside_the_tree_and_the_run_measures() {
    let fixture = Fixture::copy_with_siblings("fixture-outside-dep", &["fixture-outside-dep-lib"]);
    let sibling = fixture
        .root()
        .parent()
        .expect("the trees directory")
        .join("fixture-outside-dep-lib");
    let output = run(&fixture, &["--allow-outside", &sibling.to_string_lossy()]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("MUTANTS   cataloged="),
        "the run measured the tree: {text}"
    );
    assert!(
        !text.contains("cataloged=0"),
        "and found something to measure: {text}"
    );
}
