// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution: one test process per mutant, and what its exit status means.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::execute::{
    ExecRequest, Observation, Summary, TargetKind, TestTarget, environment, outcome_of,
    parse_summary, target_id,
};
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::EXIT_CODE_UNAVAILABLE;

// --- the libtest summary ----------------------------------------------------------

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
    // The last summary wins: a binary that printed one per suite ends with
    // the one that speaks for the run.
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

// --- outcomes ---------------------------------------------------------------------

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
    // A process that never started is an error, whatever else is true.
    let mut failed = result(EXIT_CODE_UNAVAILABLE);
    failed.unstarted = true;
    assert_eq!(outcome_of(failed, None), Outcome::Errored);

    // A timeout is a timeout, even though the exit status is unavailable.
    let mut timed_out = result(EXIT_CODE_UNAVAILABLE);
    timed_out.timed_out = true;
    assert_eq!(outcome_of(timed_out, None), Outcome::TimedOut);

    // Cancelled: no status, no timeout, no error.
    assert_eq!(
        outcome_of(result(EXIT_CODE_UNAVAILABLE), None),
        Outcome::NotRun
    );

    // A stale catalog is the engine's own fault, never a survivor.
    assert_eq!(outcome_of(result(97), Some(green())), Outcome::Errored);

    // Anything else non-zero is a failing test, which is a kill.
    assert_eq!(outcome_of(result(101), None), Outcome::Killed);
    assert_eq!(outcome_of(result(1), None), Outcome::Killed);

    // Zero with tests that ran is a survivor.
    assert_eq!(outcome_of(result(0), Some(green())), Outcome::Survived);

    // Zero with nothing run is not a survivor: nothing observed the mutant.
    let empty = Some(Summary {
        ok: true,
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 3,
    });
    assert_eq!(outcome_of(result(0), empty), Outcome::Inconclusive);

    // Zero with no summary at all: a harness that says nothing cannot be
    // read as evidence.
    assert_eq!(outcome_of(result(0), None), Outcome::Inconclusive);
}

// --- targets ------------------------------------------------------------------------

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

// --- the environment ------------------------------------------------------------------

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
    let env = environment(&base, &target(), Some(("abc", "digest")), Some(scratch));
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

    // With nothing active, the variables are absent rather than empty: an
    // empty one would be a baseline the runtime has to reason about.
    let baseline = environment(&base, &target(), None, None);
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
