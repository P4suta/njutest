// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree that reads code from beside itself: refused by name, and measured when somebody says it may.

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
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

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}
