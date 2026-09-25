// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mutating what changed: what a change set selects, and what it refuses to guess.

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};
use std::process::Output;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root.as_str()])
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

fn stdout(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
}

#[test]
fn a_change_set_selects_the_files_that_differ_and_leaves_the_rest_alone() {
    let fixture = Fixture::copy("fixture-workspace");
    njutest_devkit::repo::commit_tree(fixture.root());
    let util = fixture.root().join("crates/core/src/util.rs");
    let mut widened = std::fs::read_to_string(&util).expect("read");
    widened.push_str("\npub fn extra(a: i32) -> i32 { a + 1 }\n");
    std::fs::write(&util, widened).expect("write");

    let whole = stdout(&against(&fixture, &["list"]));
    assert!(whole.contains("crates/core/src/lib.rs"), "{whole}");
    assert!(whole.contains("crates/core/src/util.rs"), "{whole}");

    let output = against(&fixture, &["list", "--changed"]);
    let listed = stdout(&output);
    assert!(output.status.success(), "{output:?}");
    assert!(listed.contains("crates/core/src/util.rs"), "{listed}");
    assert!(
        !listed.contains("crates/core/src/lib.rs"),
        "a file that did not change is not in the change set: {listed}"
    );
}

#[test]
fn a_change_set_that_names_no_rust_file_selects_nothing_rather_than_everything() {
    let fixture = Fixture::copy("fixture-workspace");
    njutest_devkit::repo::commit_tree(fixture.root());
    std::fs::write(fixture.root().join("README.md"), "changed\n").expect("write");

    let output = against(&fixture, &["list", "--changed"]);
    let listed = stdout(&output);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !listed.contains(".rs:"),
        "a run about nothing changing must mutate nothing: {listed}"
    );
}

#[test]
fn a_run_about_a_change_that_touches_nothing_measured_says_so_and_passes() {
    let fixture = Fixture::copy("fixture-workspace");
    njutest_devkit::repo::commit_tree(fixture.root());
    std::fs::write(fixture.root().join("README.md"), "changed\n").expect("write");

    let output = against(&fixture, &["run", "--changed", "--offline", "--locked"]);
    let said = stdout(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a pull request that changed nothing this configuration measures has nothing to \
         answer for, and a gate built on --changed must be able to tell that from a failure: \
         {output:?}"
    );
    assert!(
        said.contains("NOTHING") && said.contains("README.md"),
        "it says there was nothing to measure and names what changed instead, so a \
         configuration that measures too little is visible rather than silently green: {said}"
    );
}

#[test]
fn a_tree_git_cannot_be_asked_about_ends_the_command_rather_than_reading_as_nothing_changed() {
    let fixture = Fixture::copy("fixture-workspace");
    let output = against(&fixture, &["list", "--changed"]);
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(complaint.contains("RM0010"), "{complaint}");
}

#[test]
fn a_revision_git_does_not_know_ends_the_command() {
    let fixture = Fixture::copy("fixture-workspace");
    njutest_devkit::repo::commit_tree(fixture.root());
    let output = against(&fixture, &["list", "--changed-from", "no-such-revision"]);
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(complaint.contains("no-such-revision"), "{complaint}");
}

/// The outcome of every mutant a run judged, by identity, from the lines it printed.
fn outcomes(said: &str) -> std::collections::BTreeMap<String, String> {
    said.lines()
        .filter_map(|line| line.strip_prefix('['))
        .filter_map(|line| line.split_once("] "))
        .filter_map(|(_, rest)| rest.split_once(' '))
        .map(|(id, rest)| {
            let outcome = rest.split_whitespace().next().unwrap_or_default();
            (id.to_owned(), outcome.to_owned())
        })
        .collect()
}

#[test]
fn routing_loses_no_kill_and_names_the_code_no_test_runs() {
    let fixture = Fixture::copy("fixture-workspace");
    let lib = fixture.root().join("crates/core/src/lib.rs");
    let mut widened = std::fs::read_to_string(&lib).expect("read");
    widened.push_str("\n/// Nothing calls this.\npub fn adrift(a: i32) -> i32 {\n    a + 1\n}\n");
    std::fs::write(&lib, widened).expect("write");

    let whole = stdout(&against(
        &fixture,
        &[
            "run",
            "--no-coverage",
            "--no-touch",
            "--offline",
            "--locked",
            "--no-cache",
        ],
    ));
    let routed = against(
        &fixture,
        &["run", "--coverage", "--offline", "--locked", "--no-cache"],
    );
    let said = stdout(&routed);

    let (before, after) = (outcomes(&whole), outcomes(&said));
    assert_eq!(before.len(), after.len(), "{whole}\n---\n{said}");
    for (id, outcome) in &before {
        if outcome == "killed" {
            assert_eq!(
                after.get(id).map(String::as_str),
                Some("killed"),
                "routing lost a kill: {id}"
            );
        }
    }
    let unreached: Vec<&String> = after
        .iter()
        .filter(|(_, outcome)| *outcome == "not_run")
        .map(|(id, _)| id)
        .collect();
    assert!(
        !unreached.is_empty(),
        "the function nothing calls is reached by nothing: {said}"
    );
    for id in unreached {
        assert_eq!(
            before.get(id).map(String::as_str),
            Some("survived"),
            "only a mutant that survived everything can be one nothing reached: {id}"
        );
    }
    assert!(
        said.contains("unreached-mutant"),
        "a mutant no measured test reaches is said to be one: {said}"
    );
    assert_eq!(
        routed.status.code(),
        Some(1),
        "code the tests never execute is a finding about the tests, not a broken run: {routed:?}"
    );
}
