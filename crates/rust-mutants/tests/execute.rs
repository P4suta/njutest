// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution: one test process per mutant, and what its exit status means.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::execute::{
    Context, ExecRequest, Lines, Observation, Summary, TargetKind, TestTarget, environment,
    outcome_of, parse_lines, parse_summary, target_id,
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
        stale_catalog: false,
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
    assert_eq!(outcome_of(failed, None, true), Outcome::Errored);

    let mut timed_out = result(EXIT_CODE_UNAVAILABLE);
    timed_out.timed_out = true;
    assert_eq!(outcome_of(timed_out, None, true), Outcome::TimedOut);

    assert_eq!(
        outcome_of(result(EXIT_CODE_UNAVAILABLE), None, true),
        Outcome::NotRun
    );

    assert_eq!(
        outcome_of(result(97), Some(green()), true),
        Outcome::Errored
    );

    assert_eq!(outcome_of(result(101), None, true), Outcome::Killed);
    assert_eq!(outcome_of(result(1), None, true), Outcome::Killed);

    assert_eq!(
        outcome_of(result(0), Some(green()), true),
        Outcome::Survived
    );

    let empty = Some(Summary {
        ok: true,
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 3,
    });
    assert_eq!(outcome_of(result(0), empty, true), Outcome::Inconclusive);

    assert_eq!(outcome_of(result(0), None, true), Outcome::Inconclusive);
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
    TestTarget::new(
        "demo/test/cli",
        "demo",
        TargetKind::Test,
        "cli",
        PathBuf::from("/t/debug/deps/cli-abc"),
        PathBuf::from("/w/demo"),
    )
    .with_cargo_env(vec![
        (
            OsString::from("CARGO_MANIFEST_DIR"),
            OsString::from("/w/demo"),
        ),
        (OsString::from("CARGO_PKG_NAME"), OsString::from("demo")),
    ])
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
            sysroot: None,
            active: Some(("abc", "digest")),
            probe: None,
            touch: None,
            profile: None,
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
}

#[test]
fn a_baseline_inherits_none_of_the_variables_a_run_composes_for_itself() {
    let base = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (
            OsString::from("RUST_MUTANTS_ACTIVE"),
            OsString::from("stale"),
        ),
        (
            OsString::from("RUST_MUTANTS_TOUCH"),
            OsString::from("/somebody/elses/log"),
        ),
        (OsString::from("TMPDIR"), OsString::from("/tmp")),
    ];
    let baseline = environment(
        &Context {
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: None,
            probe: None,
            touch: None,
            profile: None,
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
        "a touch log an outer run owns is one this run would append its own answers to: {names:?}"
    );
    assert!(names.iter().any(|name| name == "TMPDIR"), "{names:?}");
}

#[test]
fn the_guards_are_told_where_to_record_exactly_when_the_run_asks_them_to() {
    let log = Path::new("/scratch/touch/demo.log");
    let asked = environment(
        &Context {
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: None,
            probe: None,
            touch: Some(log),
            profile: None,
        },
        &target(),
        None,
    );
    assert!(
        asked
            .iter()
            .any(|(name, value)| name == "RUST_MUTANTS_TOUCH" && value == log.as_os_str()),
        "{asked:?}"
    );
}

#[test]
fn a_request_naming_several_tests_passes_every_one_of_them_as_an_exact_filter() {
    let target = target();
    let request = ExecRequest::new(&target).with_tests(vec![
        "tests::max_picks_the_larger".to_owned(),
        "tests::min_picks_the_smaller".to_owned(),
    ]);
    assert_eq!(
        request.argv(),
        [
            "/t/debug/deps/cli-abc",
            "tests::max_picks_the_larger",
            "tests::min_picks_the_smaller",
            "--exact",
        ],
        "libtest takes every free argument as a filter and --exact applies to all of them, so \
         one process runs exactly the tests that reached the mutation"
    );
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
    let target = TestTarget::new(
        "core/lib/core",
        "core",
        TargetKind::Lib,
        "core",
        PathBuf::from("/nowhere"),
        PathBuf::from("/nowhere"),
    );
    let composed = environment(
        &Context {
            base_env: &[],
            cargo: Some(Path::new("/opt/toolchain/bin/cargo")),
            sysroot: None,
            active: None,
            probe: None,
            touch: None,
            profile: None,
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
            sysroot: None,
            active: None,
            probe: None,
            touch: None,
            profile: None,
        },
        &target,
        None,
    );
    assert!(
        !without.iter().any(|(name, _)| name == "CARGO"),
        "a caller that names no cargo says nothing about it"
    );
}

#[test]
fn a_target_cargo_runs_puts_the_harness_arguments_after_a_separator() {
    let mut doc = target();
    doc.kind = TargetKind::ProcMacro;
    doc.executable = PathBuf::from("/bin/cargo");
    doc.through = ["test", "--doc", "--package", "demo"]
        .into_iter()
        .map(OsString::from)
        .collect();

    let argv = ExecRequest::new(&doc)
        .with_test("src/lib.rs - add (line 7)".to_owned())
        .with_args(vec!["--format".to_owned(), "terse".to_owned()])
        .argv();

    assert_eq!(
        argv,
        [
            "/bin/cargo",
            "test",
            "--doc",
            "--package",
            "demo",
            "--",
            "src/lib.rs - add (line 7)",
            "--format",
            "terse"
        ]
        .map(OsString::from),
        "cargo takes its own arguments first and passes the rest of the line to the \
         harness after a separator, and never `--exact`: rustdoc merges a file's examples \
         into one compilation, where a filter naming one of them runs all of them"
    );
}

#[test]
fn a_runtime_that_named_another_catalog_is_an_error_however_the_process_exited() {
    let said = format!(
        "{}abc but def is active\n",
        rust_mutants::instrument::STALE_CATALOG_MARKER
    );
    let observed = Observation {
        unstarted: false,
        timed_out: false,
        exit_code: 101,
        stale_catalog: said.contains(rust_mutants::instrument::STALE_CATALOG_MARKER),
    };

    assert_eq!(
        outcome_of(observed, None, true),
        Outcome::Errored,
        "cargo turns the runtime's own 97 into its 101, which is the code a failing test \
         has: a tree rebuilt behind the run's back would otherwise look exactly like a kill"
    );
}

#[test]
fn an_inherited_coverage_profile_path_never_reaches_a_test_process() {
    let base = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (
            OsString::from("LLVM_PROFILE_FILE"),
            OsString::from("default_%p.profraw"),
        ),
    ];
    let scratch = Path::new("/scratch/worker-3");
    let env = environment(
        &Context {
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: Some(("abc", "digest")),
            probe: None,
            touch: None,
            profile: None,
        },
        &target(),
        Some(scratch),
    );
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    let profile = lookup("LLVM_PROFILE_FILE").expect("a path of the run's own");
    assert_ne!(
        profile, "default_%p.profraw",
        "an inherited path would write over the measurement that started the run"
    );
    assert!(
        profile.starts_with("/scratch/worker-3"),
        "and an instrumented binary with no path writes default_*.profraw into its working \
         directory, which is the tree being measured: {profile}"
    );
    assert_eq!(lookup("PATH").as_deref(), Some("/usr/bin"));
}

#[test]
fn the_profile_path_a_coverage_pass_composes_is_the_one_it_gets() {
    let base = vec![(
        OsString::from("LLVM_PROFILE_FILE"),
        OsString::from("inherited.profraw"),
    )];
    let mine = Path::new("/scratch/coverage/demo-%m.profraw");
    let env = environment(
        &Context {
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: None,
            probe: None,
            touch: None,
            profile: Some(mine),
        },
        &target(),
        None,
    );
    let value = env
        .iter()
        .find(|(name, _)| name == "LLVM_PROFILE_FILE")
        .map(|(_, value)| value.clone());
    assert_eq!(value.as_deref(), Some(mine.as_os_str()));
}

#[test]
fn a_test_target_built_step_by_step_equals_the_literal_it_replaces() {
    let built = TestTarget::new(
        "demo/lib/demo",
        "demo",
        TargetKind::Lib,
        "demo",
        PathBuf::from("/w/target/debug/deps/demo-1"),
        PathBuf::from("/w/demo"),
    )
    .with_cargo_env(vec![(
        OsString::from("CARGO_MANIFEST_DIR"),
        OsString::from("/w/demo"),
    )])
    .with_through(vec![OsString::from("test"), OsString::from("--doc")]);
    assert_eq!(built.id, "demo/lib/demo");
    assert_eq!(built.package, "demo");
    assert_eq!(built.kind, TargetKind::Lib);
    assert_eq!(built.name, "demo");
    assert_eq!(built.cwd, PathBuf::from("/w/demo"));
    assert_eq!(built.cargo_env.len(), 1);
    assert_eq!(built.through.len(), 2);

    let plain = TestTarget::new(
        "demo/lib/demo",
        "demo",
        TargetKind::Lib,
        "demo",
        PathBuf::from("/w/target/debug/deps/demo-1"),
        PathBuf::from("/w/demo"),
    );
    assert!(
        plain.cargo_env.is_empty() && plain.through.is_empty(),
        "what a builder was not told stays empty rather than being guessed"
    );
}

#[test]
fn the_environment_reproduces_cargos_documented_set() {
    let package = rust_mutants::cargo::Package {
        id: "demo 0.1.0".to_owned(),
        name: "demo".to_owned(),
        version: "1.2.3-rc.4".to_owned(),
        manifest_path: PathBuf::from("/w/demo/Cargo.toml"),
        edition: "2024".to_owned(),
        targets: Vec::new(),
        dependencies: Vec::new(),
        authors: vec!["A Person <a@example.invalid>".to_owned(), "B".to_owned()],
        description: Some("what it is".to_owned()),
        homepage: Some("https://example.invalid".to_owned()),
        repository: Some("https://example.invalid/repo".to_owned()),
        license: Some("MIT OR Apache-2.0".to_owned()),
        license_file: Some(PathBuf::from("LICENSE")),
        rust_version: Some("1.98".to_owned()),
        readme: Some(PathBuf::from("README.md")),
    };
    let env = rust_mutants::execute::package_environment(&package);
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    for (name, value) in [
        ("CARGO_PKG_NAME", "demo"),
        ("CARGO_PKG_VERSION", "1.2.3-rc.4"),
        ("CARGO_PKG_VERSION_MAJOR", "1"),
        ("CARGO_PKG_VERSION_MINOR", "2"),
        ("CARGO_PKG_VERSION_PATCH", "3"),
        ("CARGO_PKG_VERSION_PRE", "rc.4"),
        ("CARGO_PKG_AUTHORS", "A Person <a@example.invalid>:B"),
        ("CARGO_PKG_DESCRIPTION", "what it is"),
        ("CARGO_PKG_HOMEPAGE", "https://example.invalid"),
        ("CARGO_PKG_REPOSITORY", "https://example.invalid/repo"),
        ("CARGO_PKG_LICENSE", "MIT OR Apache-2.0"),
        ("CARGO_PKG_LICENSE_FILE", "LICENSE"),
        ("CARGO_PKG_RUST_VERSION", "1.98"),
        ("CARGO_PKG_README", "README.md"),
    ] {
        assert_eq!(
            lookup(name).as_deref(),
            Some(value),
            "cargo sets {name}, and a test that reads it back gets the empty string when the \
             run did not"
        );
    }
}

#[test]
fn a_package_that_says_nothing_about_itself_still_sets_what_cargo_sets() {
    let package = rust_mutants::cargo::Package {
        id: "demo 0.1.0".to_owned(),
        name: "demo".to_owned(),
        version: "0.1.0".to_owned(),
        manifest_path: PathBuf::from("/w/demo/Cargo.toml"),
        edition: "2024".to_owned(),
        targets: Vec::new(),
        dependencies: Vec::new(),
        authors: Vec::new(),
        description: None,
        homepage: None,
        repository: None,
        license: None,
        license_file: None,
        rust_version: None,
        readme: None,
    };
    let env = rust_mutants::execute::package_environment(&package);
    let names: Vec<String> = env
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    for name in [
        "CARGO_PKG_DESCRIPTION",
        "CARGO_PKG_LICENSE",
        "CARGO_PKG_README",
    ] {
        assert!(
            names.contains(&name.to_owned()),
            "cargo sets it to the empty string rather than leaving it out, and a test that \
             reads it back must see what cargo would show it: {names:?}"
        );
    }
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    assert_eq!(lookup("CARGO_PKG_VERSION_PRE").as_deref(), Some(""));
    assert_eq!(lookup("CARGO_PKG_AUTHORS").as_deref(), Some(""));
}

#[test]
fn a_custom_harness_that_exits_zero_survived_and_one_that_exits_nonzero_killed() {
    let ran = |exit_code: i32| {
        outcome_of(
            Observation {
                unstarted: false,
                timed_out: false,
                exit_code,
                stale_catalog: false,
            },
            None,
            false,
        )
    };
    assert_eq!(
        ran(0),
        Outcome::Survived,
        "a target with no libtest harness prints what it likes and says what it found by \
         exiting, so a zero is a test suite that passed with the mutation active"
    );
    assert_eq!(ran(1), Outcome::Killed);
    assert_eq!(ran(101), Outcome::Killed);
}

#[test]
fn a_libtest_target_that_printed_no_summary_is_undecided_rather_than_survived() {
    let silent = outcome_of(
        Observation {
            unstarted: false,
            timed_out: false,
            exit_code: 0,
            stale_catalog: false,
        },
        None,
        true,
    );
    assert_eq!(
        silent,
        Outcome::Inconclusive,
        "a libtest binary that exits zero and prints no summary ran nothing anybody can point \
         at, and calling that a survivor claims a test passed that nobody saw"
    );
}

#[test]
fn silence_is_decided_by_the_harness() {
    for (outcome, answers, why) in [
        (
            Outcome::Inconclusive,
            false,
            "a libtest target that ran no test, or printed no summary: it said nothing anybody \
             can point at, and treating that as an answer hides every target after it",
        ),
        (Outcome::Survived, true, "a test ran and passed"),
        (Outcome::Killed, true, "a target that failed"),
        (Outcome::TimedOut, true, "a target that never returned"),
        (Outcome::Errored, true, "a harness that failed"),
        (Outcome::NotRun, true, "a target nothing reached"),
    ] {
        assert_eq!(rust_mutants::execute::answered(outcome), answers, "{why}");
    }
}

#[test]
fn parse_lines_names_every_test_and_its_verdict() {
    let output = b"\nrunning 4 tests\ntest tests::adds ... ok\ntest tests::subtracts ... FAILED\ntest tests::skipped ... ignored\ntest tests::explained ... ignored, needs a network\n\nfailures:\n\n---- tests::subtracts stdout ----\nassertion failed\n\nfailures:\n    tests::subtracts\n\ntest result: FAILED. 1 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out\n";
    let lines = parse_lines(output);
    assert_eq!(lines.passed, ["tests::adds"]);
    assert_eq!(
        lines.failed,
        ["tests::subtracts"],
        "the failures block names the same test again, and a name is one test however often \
         the harness prints it"
    );
    assert_eq!(lines.ignored, ["tests::skipped", "tests::explained"]);
}

#[test]
fn a_documented_example_is_named_the_way_rustdoc_names_it() {
    let lines = parse_lines(b"test src/lib.rs - max (line 9) ... ok\n");
    assert_eq!(lines.passed, ["src/lib.rs - max (line 9)"]);
}

#[test]
fn a_line_that_is_not_a_verdict_is_not_a_test() {
    let lines = parse_lines(
        b"test result: ok. 1 passed; 0 failed\nrunning 1 test\ntesting the water ... ok\n",
    );
    assert_eq!(lines, Lines::default());
}

#[test]
fn a_probe_process_that_could_not_record_is_errored() {
    let observed = Observation {
        unstarted: false,
        timed_out: false,
        exit_code: rust_mutants::probe::runtime::UNAVAILABLE_EXIT,
        stale_catalog: false,
    };
    assert_eq!(
        outcome_of(observed, None, true),
        Outcome::Errored,
        "a process that says it could not record what it saw has said nothing about the \
         mutation, and reading its exit as a kill would credit a test that never ran"
    );
}

proptest::proptest! {
    /// Nothing a harness can print makes the reader panic or invent a test.
    ///
    /// The output a run keeps is the tail of what the process wrote, so the
    /// reader meets half lines, interleaved lines, and whatever a test printed
    /// on purpose. Every name it reports has to come from a line that is
    /// there, because a name in a report is a sentence somebody will act on.
    #[test]
    fn parse_lines_never_panics_and_never_reports_more_than_the_lines(
        text in "(test [a-z:_ ]{0,12} \\.\\.\\. (ok|FAILED|ignored)\n|[a-zA-Z:. \n]{0,40}){0,8}"
    ) {
        let lines = parse_lines(text.as_bytes());
        let counted = lines
            .passed
            .len()
            .saturating_add(lines.failed.len())
            .saturating_add(lines.ignored.len());
        proptest::prop_assert!(
            counted <= text.lines().count(),
            "{counted} verdicts from {} lines of {text:?}",
            text.lines().count()
        );
        for name in lines.passed.iter().chain(&lines.failed).chain(&lines.ignored) {
            proptest::prop_assert!(
                text.contains(name.as_str()),
                "{name:?} is in no line of {text:?}"
            );
        }
    }
}
