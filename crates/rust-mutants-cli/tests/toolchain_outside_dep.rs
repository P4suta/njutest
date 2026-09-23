// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree that reads code from beside itself: refused by name, and measured when somebody says it may.

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
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
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
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
    let output = run(
        &fixture,
        &["--allow-outside", njutest_devkit::paths::utf8(&sibling)],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        text.contains("mutants were cataloged"),
        "the run measured the tree: {text}"
    );
    assert!(
        !text.contains("MUTANTS    0 mutants"),
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

#[test]
fn an_allowed_dependency_that_climbs_more_than_one_level_is_measured() {
    let fixture =
        Fixture::copy_with_siblings("nested/fixture-climbs-dep", &["fixture-climbs-dep-lib"]);
    let sibling = fixture
        .root()
        .parent()
        .expect("the trees directory")
        .parent()
        .expect("the temporary trees root")
        .join("fixture-climbs-dep-lib");
    let output = run(
        &fixture,
        &["--allow-outside", njutest_devkit::paths::utf8(&sibling)],
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let err = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "the run measures the tree: {err}"
    );
    assert!(
        text.contains("mutants were cataloged"),
        "the climbing dependency resolved inside the copy: {text}{err}"
    );
}
