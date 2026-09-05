// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution: one test process per mutant, and what its exit status means.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::execute::{
    Context, ExecRequest, Observation, Summary, TargetKind, TestTarget, environment, outcome_of,
    parse_summary, target_id,
};
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::EXIT_CODE_UNAVAILABLE;

#[test]
fn the_summary_line_is_read_whatever_the_counts_say() {
    let cases: [(&str, Option<Summary>); 6] = [
        (
            "\nrunning 3 tests\ntest a ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\n",
            Some(Summary {
                ok: true,
                passed: 3,
                failed: 0,
                ignored: 0,
                measured: 0,
                filtered_out: 0,
            }),
        ),
        (
            "test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.02s\n",
            Some(Summary {
                ok: false,
                passed: 1,
                failed: 2,
                ignored: 0,
                measured: 0,
                filtered_out: 5,
            }),
        ),
        (
            "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
            Some(Summary {
                ok: true,
                passed: 0,
                failed: 0,
                ignored: 1,
                measured: 0,
                filtered_out: 0,
            }),
        ),
        (
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.00s\n",
            Some(Summary {
                ok: true,
                passed: 0,
                failed: 0,
                ignored: 0,
                measured: 0,
                filtered_out: 7,
            }),
        ),
        ("running 0 tests\n", None),
        ("", None),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_summary(text.as_bytes()), expected, "{text:?}");
    }
    let two = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    assert_eq!(parse_summary(two.as_bytes()).expect("a summary").failed, 1);
}

#[test]
fn a_summary_counts_what_ran() {
    let summary = Summary {
        ok: true,
        passed: 2,
        failed: 1,
        ignored: 3,
        measured: 0,
        filtered_out: 4,
    };
    assert_eq!(summary.tests_run(), 3, "passed and failed, not ignored");
    assert!(!summary.ran_nothing());
    let nothing = Summary {
        ok: true,
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 9,
    };
    assert!(
        nothing.ran_nothing(),
        "a filter that matched nothing is green and empty"
    );
}

const fn result(exit_code: i32) -> Observation {
    Observation {
        unstarted: false,
        timed_out: false,
        exit_code,
    }
}

const fn green() -> Summary {
    Summary {
        ok: true,
        passed: 1,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
    }
}

#[test]
fn the_exit_status_is_read_in_one_fixed_order() {
    let mut failed = result(EXIT_CODE_UNAVAILABLE);
    failed.unstarted = true;
    assert_eq!(outcome_of(failed, None), Outcome::Errored);

    let mut timed_out = result(EXIT_CODE_UNAVAILABLE);
    timed_out.timed_out = true;
    assert_eq!(outcome_of(timed_out, None), Outcome::TimedOut);

    assert_eq!(
        outcome_of(result(EXIT_CODE_UNAVAILABLE), None),
        Outcome::NotRun
    );

    assert_eq!(outcome_of(result(97), Some(green())), Outcome::Errored);

    assert_eq!(outcome_of(result(101), None), Outcome::Killed);
    assert_eq!(outcome_of(result(1), None), Outcome::Killed);

    assert_eq!(outcome_of(result(0), Some(green())), Outcome::Survived);

    let empty = Some(Summary {
        ok: true,
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 3,
    });
    assert_eq!(outcome_of(result(0), empty), Outcome::Inconclusive);

    assert_eq!(outcome_of(result(0), None), Outcome::Inconclusive);
}

#[test]
fn a_target_is_named_by_package_kind_and_name() {
    assert_eq!(target_id("demo", TargetKind::Lib, "demo"), "demo/lib/demo");
    assert_eq!(target_id("demo", TargetKind::Test, "cli"), "demo/test/cli");
    assert_eq!(target_id("demo", TargetKind::Bin, "demo"), "demo/bin/demo");
    assert_eq!(
        target_id("demo", TargetKind::Example, "usage"),
        "demo/example/usage"
    );
    for kind in TargetKind::ALL {
        assert!(!kind.name().is_empty());
        assert_eq!(TargetKind::parse(kind.name()), Some(kind));
    }
    assert_eq!(TargetKind::parse("bench"), None);
}

fn target() -> TestTarget {
    TestTarget {
        id: "demo/test/cli".to_owned(),
        package: "demo".to_owned(),
        kind: TargetKind::Test,
        name: "cli".to_owned(),
        executable: PathBuf::from("/t/debug/deps/cli-abc"),
        cwd: PathBuf::from("/w/demo"),
        cargo_env: vec![
            (
                OsString::from("CARGO_MANIFEST_DIR"),
                OsString::from("/w/demo"),
            ),
            (OsString::from("CARGO_PKG_NAME"), OsString::from("demo")),
        ],
    }
}

#[test]
fn the_environment_is_the_base_plus_cargos_own_plus_the_activation() {
    let base = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (
            OsString::from("RUST_MUTANTS_ACTIVE"),
            OsString::from("stale"),
        ),
        (
            OsString::from("RUST_MUTANTS_PROBE"),
            OsString::from("stale"),
        ),
        (OsString::from("TMPDIR"), OsString::from("/tmp")),
    ];
    let scratch = Path::new("/scratch/worker-3");
    let env = environment(
        &Context {
            base_env: &base,
            cargo: None,
            active: Some(("abc", "digest")),
        },
        &target(),
        Some(scratch),
    );
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    assert_eq!(lookup("PATH").as_deref(), Some("/usr/bin"));
    assert_eq!(lookup("CARGO_MANIFEST_DIR").as_deref(), Some("/w/demo"));
    assert_eq!(lookup("CARGO_PKG_NAME").as_deref(), Some("demo"));
    assert_eq!(lookup("RUST_MUTANTS_ACTIVE").as_deref(), Some("abc"));
    assert_eq!(lookup("RUST_MUTANTS_CATALOG").as_deref(), Some("digest"));
    assert_eq!(
        lookup("RUST_MUTANTS_PROBE"),
        None,
        "a stale probe variable is removed, never inherited"
    );
    for key in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(
            lookup(key).as_deref(),
            Some("/scratch/worker-3"),
            "{key} points at the worker's own scratch"
        );
    }
    let names: Vec<String> = env
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        names.len(),
        sorted.len(),
        "no name appears twice: {names:?}"
    );

    let baseline = environment(
        &Context {
            base_env: &base,
            cargo: None,
            active: None,
        },
        &target(),
        None,
    );
    let names: Vec<String> = baseline
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.iter().any(|name| name.starts_with("RUST_MUTANTS_")),
        "{names:?}"
    );
    assert!(names.iter().any(|name| name == "TMPDIR"), "{names:?}");
}

#[test]
fn a_request_names_the_arguments_the_binary_receives() {
    let target = target();
    let request = ExecRequest::new(&target).with_test("tests::max_picks_the_larger");
    assert_eq!(
        request.argv(),
        [
            "/t/debug/deps/cli-abc",
            "tests::max_picks_the_larger",
            "--exact",
        ]
    );
    let plain = ExecRequest::new(&target);
    assert_eq!(plain.argv(), ["/t/debug/deps/cli-abc"]);
    let with_args = ExecRequest::new(&target)
        .with_test("a::b")
        .with_args(["--test-threads=1".to_owned()]);
    assert_eq!(
        with_args.argv(),
        [
            "/t/debug/deps/cli-abc",
            "a::b",
            "--exact",
            "--test-threads=1"
        ]
    );
}

#[test]
fn a_test_process_learns_which_cargo_built_it() {
    let target = TestTarget {
        id: "core/lib/core".to_owned(),
        package: "core".to_owned(),
        kind: TargetKind::Lib,
        name: "core".to_owned(),
        executable: PathBuf::from("/nowhere"),
        cwd: PathBuf::from("/nowhere"),
        cargo_env: Vec::new(),
    };
    let composed = environment(
        &Context {
            base_env: &[],
            cargo: Some(Path::new("/opt/toolchain/bin/cargo")),
            active: None,
        },
        &target,
        None,
    );
    let cargo = composed
        .iter()
        .find(|(name, _)| name == "CARGO")
        .map(|(_, value)| value.clone());
    assert_eq!(
        cargo,
        Some(OsString::from("/opt/toolchain/bin/cargo")),
        "cargo sets CARGO to an absolute path for every process it runs, and a test that \
         spawns cargo reads it"
    );

    let without = environment(
        &Context {
            base_env: &[],
            cargo: None,
            active: None,
        },
        &target,
        None,
    );
    assert!(
        !without.iter().any(|(name, _)| name == "CARGO"),
        "a caller that names no cargo says nothing about it"
    );
}
