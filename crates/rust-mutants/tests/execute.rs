// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution: one test process per mutant, and what its exit status means.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::execute::{
    Context, ExecRequest, Lines, Observation, StartFailure, StepLimitNotice, StepProtocolFailure,
    Stopped, Summary, TargetKind, TestTarget, environment, outcome_of, parse_lines, parse_summary,
    target_id,
};
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::ProcessExit;

fn exact_os_text(value: &OsStr) -> String {
    let exact = value.to_str();
    assert!(exact.is_some(), "test environment values are exact UTF-8");
    let Some(exact) = exact else {
        std::process::abort()
    };
    exact.to_owned()
}

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
        let parsed = parse_summary(text.as_bytes());
        assert_eq!(
            result_state(&parsed),
            Returned,
            "the fixture is exact UTF-8"
        );
        let Ok(parsed) = parsed else { return };
        assert_eq!(parsed, expected, "{text:?}");
    }
    let two = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    let parsed = parse_summary(two.as_bytes());
    assert_eq!(
        result_state(&parsed),
        Returned,
        "the fixture is exact UTF-8"
    );
    let Ok(parsed) = parsed else { return };
    assert!(parsed.is_some(), "a summary");
    let Some(parsed) = parsed else { return };
    assert_eq!(parsed.failed, 1);
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
    assert_eq!(
        summary.tests_run(),
        Some(3),
        "passed and failed, not ignored"
    );
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
        stopped: Stopped::Exited {
            exit: ProcessExit::Code(exit_code),
        },
        stale_catalog: false,
    }
}

#[cfg(unix)]
const fn signalled(signal: i32) -> Observation {
    Observation {
        stopped: Stopped::Exited {
            exit: ProcessExit::Signal(signal),
        },
        stale_catalog: false,
    }
}

const fn stopped(stopped: Stopped) -> Observation {
    Observation {
        stopped,
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

fn step_notice() -> StepLimitNotice {
    StepLimitNotice::specimen()
}

#[test]
fn the_exit_status_is_read_in_one_fixed_order() {
    assert_eq!(
        outcome_of(
            &stopped(Stopped::NotStarted {
                cause: StartFailure::Missing
            }),
            None,
            (true, &[])
        ),
        Outcome::Errored
    );

    assert_eq!(
        outcome_of(
            &stopped(Stopped::TimedOut { raised: None }),
            None,
            (true, &[])
        ),
        Outcome::Waited,
        "a bound expiring establishes that this machine stopped waiting, which is not a \
         thing the tests did"
    );

    assert_eq!(
        outcome_of(
            &stopped(Stopped::StepLimitReached {
                notice: step_notice(),
            }),
            None,
            (true, &[]),
        ),
        Outcome::StepLimitReached
    );

    assert_eq!(
        outcome_of(
            &stopped(Stopped::Exited {
                exit: ProcessExit::Unknown,
            }),
            None,
            (true, &[]),
        ),
        Outcome::NotRun
    );

    assert_eq!(
        outcome_of(&result(95), Some(green()), (true, &[])),
        Outcome::Killed,
        "a bare status formerly reserved by the runtime is only a nonzero process exit; the \
         nonce-bound notice, not a colliding number, establishes the step fact"
    );

    assert_eq!(outcome_of(&result(101), None, (true, &[])), Outcome::Killed);
    assert_eq!(outcome_of(&result(1), None, (true, &[])), Outcome::Killed);

    assert_eq!(
        outcome_of(&result(0), Some(green()), (true, &[])),
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
    assert_eq!(
        outcome_of(&result(0), empty, (true, &[])),
        Outcome::Inconclusive
    );

    assert_eq!(
        outcome_of(&result(0), None, (true, &[])),
        Outcome::Inconclusive
    );
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
            OsString::from("RUST_MUTANTS_TOUCH"),
            OsString::from("stale"),
        ),
        (OsString::from("TMPDIR"), OsString::from("/tmp")),
    ];
    let scratch = Path::new("/scratch/worker-3");
    let env = environment(
        &Context {
            leaders: None,
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: Some(("abc", "digest")),
            beside: None,
            touch: None,
            steps: None,
            profile: None,
            crash: None,
        },
        &target(),
        (Some(scratch), Some(scratch)),
    );
    assert_eq!(
        result_state(&env),
        Returned,
        "compose the execution environment: {env:?}"
    );
    let Ok(env) = env else { return };
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| exact_os_text(value))
    };
    assert_eq!(lookup("PATH").as_deref(), Some("/usr/bin"));
    assert_eq!(lookup("CARGO_MANIFEST_DIR").as_deref(), Some("/w/demo"));
    assert_eq!(lookup("CARGO_PKG_NAME").as_deref(), Some("demo"));
    assert_eq!(lookup("RUST_MUTANTS_ACTIVE").as_deref(), Some("abc"));
    assert_eq!(lookup("RUST_MUTANTS_CATALOG").as_deref(), Some("digest"));
    assert_eq!(
        lookup("RUST_MUTANTS_TOUCH"),
        None,
        "a stale record variable is removed, never inherited"
    );
    for key in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(
            lookup(key).as_deref(),
            Some("/scratch/worker-3"),
            "{key} points at the worker's own scratch"
        );
    }
    let names: Vec<String> = env.iter().map(|(name, _)| exact_os_text(name)).collect();
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
        (
            OsString::from(if cfg!(windows) {
                "rust_mutants_steps"
            } else {
                "RUST_MUTANTS_STEPS"
            }),
            OsString::from("9"),
        ),
        (OsString::from("TMPDIR"), OsString::from("/tmp")),
    ];
    let baseline = environment(
        &Context {
            leaders: None,
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: None,
            beside: None,
            touch: None,
            steps: None,
            profile: None,
            crash: None,
        },
        &target(),
        (None, None),
    );
    assert_eq!(
        result_state(&baseline),
        Returned,
        "compose the baseline environment: {baseline:?}"
    );
    let Ok(baseline) = baseline else { return };
    let names: Vec<String> = baseline
        .iter()
        .map(|(name, _)| exact_os_text(name))
        .collect();
    assert!(
        !baseline.iter().any(|(name, _)| {
            rust_mutants::execute::COMPOSED_ENV
                .iter()
                .any(|composed| rust_mutants::vars::same_name(name, OsStr::new(composed)))
        }),
        "a composed variable spelled the way the platform takes for the same name is the same \
         variable, and no test process inherits it either: {names:?}"
    );
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
            leaders: None,
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: None,
            beside: None,
            touch: Some(rust_mutants::execute::Touching {
                scope: rust_mutants::execute::TouchScope::Everything,
                log,
                catalog: "digest",
            }),
            steps: None,
            profile: None,
            crash: None,
        },
        &target(),
        (None, None),
    );
    assert_eq!(
        result_state(&asked),
        Returned,
        "compose the touch environment: {asked:?}"
    );
    let Ok(asked) = asked else { return };
    assert!(
        asked
            .iter()
            .any(|(name, value)| name == "RUST_MUTANTS_TOUCH" && value == log.as_os_str()),
        "{asked:?}"
    );
    assert!(
        asked
            .iter()
            .any(|(name, value)| name == "RUST_MUTANTS_CATALOG" && value == "digest"),
        "a record is about one catalog, and a process records into it only when it was built \
         from that one: {asked:?}"
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
        "core",
        TargetKind::Lib,
        "core",
        PathBuf::from("/nowhere"),
        PathBuf::from("/nowhere"),
    );
    let composed = environment(
        &Context {
            leaders: None,
            base_env: &[],
            cargo: Some(Path::new("/opt/toolchain/bin/cargo")),
            sysroot: None,
            active: None,
            beside: None,
            touch: None,
            steps: None,
            profile: None,
            crash: None,
        },
        &target,
        (None, None),
    );
    assert_eq!(
        result_state(&composed),
        Returned,
        "compose the cargo environment: {composed:?}"
    );
    let Ok(composed) = composed else { return };
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
            leaders: None,
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: None,
            beside: None,
            touch: None,
            steps: None,
            profile: None,
            crash: None,
        },
        &target,
        (None, None),
    );
    assert_eq!(
        result_state(&without),
        Returned,
        "compose the environment without cargo: {without:?}"
    );
    let Ok(without) = without else { return };
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
        stopped: Stopped::Exited {
            exit: ProcessExit::Code(101),
        },
        stale_catalog: said.contains(rust_mutants::instrument::STALE_CATALOG_MARKER),
    };

    assert_eq!(
        outcome_of(&observed, None, (true, &[])),
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
            leaders: None,
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: Some(("abc", "digest")),
            beside: None,
            touch: None,
            steps: None,
            profile: None,
            crash: None,
        },
        &target(),
        (Some(scratch), Some(scratch)),
    );
    assert_eq!(
        result_state(&env),
        Returned,
        "compose the instrumented environment: {env:?}"
    );
    let Ok(env) = env else { return };
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| exact_os_text(value))
    };
    let profile = lookup("LLVM_PROFILE_FILE");
    assert!(profile.is_some(), "a path of the run's own");
    let Some(profile) = profile else { return };
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
            leaders: None,
            base_env: &base,
            cargo: None,
            sysroot: None,
            active: None,
            beside: None,
            touch: None,
            steps: None,
            profile: Some(mine),
            crash: None,
        },
        &target(),
        (None, None),
    );
    assert_eq!(
        result_state(&env),
        Returned,
        "compose the coverage environment: {env:?}"
    );
    let Ok(env) = env else { return };
    let value = env
        .iter()
        .find(|(name, _)| name == "LLVM_PROFILE_FILE")
        .map(|(_, value)| value.clone());
    assert_eq!(value.as_deref(), Some(mine.as_os_str()));
}

#[test]
fn a_test_target_built_step_by_step_equals_the_literal_it_replaces() {
    let built = TestTarget::new(
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
    let package = serde_json::from_value::<rust_mutants::cargo::Package>(serde_json::json!({
        "id": "demo 0.1.0",
        "name": "demo",
        "version": "1.2.3-rc.4",
        "manifest_path": "/w/demo/Cargo.toml",
        "edition": "2024",
        "targets": [],
        "dependencies": [],
        "authors": ["A Person <a@example.invalid>", "B"],
        "description": "what it is",
        "homepage": "https://example.invalid",
        "repository": "https://example.invalid/repo",
        "license": "MIT OR Apache-2.0",
        "license_file": "LICENSE",
        "rust_version": "1.98",
        "readme": "README.md",
    }));
    assert_eq!(
        result_state(&package),
        Returned,
        "the package is one: {package:?}"
    );
    let Ok(package) = package else { return };
    let env = rust_mutants::execute::package_environment(&package);
    let lookup = |key: &str| -> Option<String> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| exact_os_text(value))
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
    let package = serde_json::from_value::<rust_mutants::cargo::Package>(serde_json::json!({
        "id": "demo 0.1.0",
        "name": "demo",
        "version": "0.1.0",
        "manifest_path": "/w/demo/Cargo.toml",
        "edition": "2024",
        "targets": [],
        "dependencies": [],
        "authors": [],
    }));
    assert_eq!(
        result_state(&package),
        Returned,
        "the package is one: {package:?}"
    );
    let Ok(package) = package else { return };
    let env = rust_mutants::execute::package_environment(&package);
    let names: Vec<String> = env.iter().map(|(name, _)| exact_os_text(name)).collect();
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
            .map(|(_, value)| exact_os_text(value))
    };
    assert_eq!(lookup("CARGO_PKG_VERSION_PRE").as_deref(), Some(""));
    assert_eq!(lookup("CARGO_PKG_AUTHORS").as_deref(), Some(""));
}

#[test]
fn a_custom_harness_that_exits_zero_survived_and_one_that_exits_nonzero_killed() {
    let ran = |exit_code: i32| {
        outcome_of(
            &Observation {
                stopped: Stopped::Exited {
                    exit: ProcessExit::Code(exit_code),
                },
                stale_catalog: false,
            },
            None,
            (false, &[]),
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
        &Observation {
            stopped: Stopped::Exited {
                exit: ProcessExit::Code(0),
            },
            stale_catalog: false,
        },
        None,
        (true, &[]),
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
        (
            Outcome::StepLimitReached,
            false,
            "reaching a finite guard allowance leaves the mutation unanswered",
        ),
        (
            Outcome::Waited,
            true,
            "a target this machine stopped waiting for",
        ),
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
    assert_eq!(result_state(&lines), Returned, "the fixture is exact UTF-8");
    let Ok(lines) = lines else { return };
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
    assert_eq!(result_state(&lines), Returned, "the fixture is exact UTF-8");
    let Ok(lines) = lines else { return };
    assert_eq!(lines.passed, ["src/lib.rs - max (line 9)"]);
}

#[test]
fn a_test_that_expects_a_panic_is_named_by_its_name_and_not_by_what_libtest_adds_to_it() {
    let lines = parse_lines(
        b"test tests::refuses - should panic ... ok\ntest tests::stops - should panic ... FAILED\n",
    );
    assert_eq!(result_state(&lines), Returned, "the fixture is exact UTF-8");
    let Ok(lines) = lines else { return };
    assert_eq!(
        lines.passed,
        ["tests::refuses"],
        "the thread libtest runs the test on, and the filter that selects it, both know it by \
         its name"
    );
    assert_eq!(lines.failed, ["tests::stops"]);
}

#[test]
fn a_line_that_is_not_a_verdict_is_not_a_test() {
    let lines = parse_lines(
        b"test result: ok. 1 passed; 0 failed\nrunning 1 test\ntesting the water ... ok\n",
    );
    assert_eq!(result_state(&lines), Returned, "the fixture is exact UTF-8");
    let Ok(lines) = lines else { return };
    assert_eq!(lines, Lines::default());
}

proptest::proptest! {
    /// Nothing a harness can print makes the reader panic or invent a test.
    #[test]
    fn parse_lines_never_panics_and_never_reports_more_than_the_lines(
        text in "(test [a-z:_ ]{0,12} \\.\\.\\. (ok|FAILED|ignored)\n|[a-zA-Z:. \n]{0,40}){0,8}"
    ) {
        let lines = parse_lines(text.as_bytes());
        proptest::prop_assert_eq!(
            result_state(&lines),
            Returned,
            "the generator emits exact UTF-8: {:?}",
            lines
        );
        let Ok(lines) = lines else { return Ok(()) };
        let counted = lines.passed.len()
            .checked_add(lines.failed.len())
            .and_then(|count| count.checked_add(lines.ignored.len()));
        proptest::prop_assert!(
            counted.is_some(),
            "the generated fixture is bounded to eight lines"
        );
        let Some(counted) = counted else { return Ok(()) };
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

#[test]
fn a_clock_that_ended_a_counting_computation_says_so_and_one_that_ended_a_silent_one_says_that() {
    for (raised, what) in [
        (
            Some(0_u64),
            "no boundary was raised, so no allowance could have ended it",
        ),
        (
            Some(41),
            "the allowance would have ended this and the clock got there first",
        ),
        (
            None,
            "the state could not be read, which is not a count of zero",
        ),
    ] {
        assert_eq!(
            outcome_of(&stopped(Stopped::TimedOut { raised }), None, (true, &[])),
            Outcome::Waited,
            "the verdict does not move, because the count did not fire: {what}"
        );
    }
    assert_ne!(
        Stopped::TimedOut { raised: Some(0) },
        Stopped::TimedOut { raised: Some(41) },
        "and the two stop being one word. A clock that ended a computation raising the count \
         ended one the allowance would have ended, which is a race this machine won and \
         another would not, and the number to change is the allowance. A clock that ended \
         one raising nothing is the only instrument there is (ADR 0023), and there is \
         nothing to change"
    );
}

#[test]
fn a_test_the_harness_said_failed_is_a_detection_whatever_the_clock_did_afterwards() {
    let failed = vec!["a_says_the_answer_is_ready".to_owned()];
    for stop in [
        Stopped::TimedOut { raised: None },
        Stopped::Stalled { raised: None },
    ] {
        assert_eq!(
            outcome_of(&stopped(stop.clone()), None, (true, &failed)),
            Outcome::Killed,
            "a test that finished and failed noticed the mutation; that another test then kept \
             the process running until the clock stopped it is a fact about the process: {stop:?}"
        );
        assert_eq!(
            outcome_of(&stopped(stop), None, (true, &[])),
            Outcome::Waited,
            "and with nothing failed, the clock stopping it establishes only that it stopped"
        );
    }
    assert_eq!(
        outcome_of(
            &stopped(Stopped::TimedOut { raised: None }),
            None,
            (false, &failed)
        ),
        Outcome::Waited,
        "a harness that is not libtest names no failure this reader can trust"
    );
}

#[cfg(unix)]
#[test]
fn a_signal_sent_from_outside_is_no_detection_and_one_the_process_raised_is() {
    for (signal, name) in [(1, "HUP"), (2, "INT"), (9, "KILL"), (15, "TERM")] {
        assert_ne!(
            outcome_of(&signalled(signal), None, (true, &[])),
            Outcome::Killed,
            "SIG{name} is what a cancelled CI job or an out-of-memory killer sends; the tests \
             did not notice anything, and a kill stored from it would hide a survivor from \
             every later run that reads it back"
        );
    }
    for (signal, name) in [
        (4, "ILL"),
        (5, "TRAP"),
        (6, "ABRT"),
        (8, "FPE"),
        (11, "SEGV"),
    ] {
        assert_eq!(
            outcome_of(&signalled(signal), None, (true, &[])),
            Outcome::Killed,
            "SIG{name} is raised by what the process itself did, which a mutation can make it do"
        );
    }
    let failed = vec!["a_test_that_noticed".to_owned()];
    assert_eq!(
        outcome_of(&signalled(9), None, (true, &failed)),
        Outcome::Killed,
        "a test the harness had already said failed noticed the mutation, whatever ended the \
         process afterwards"
    );
}

#[test]
fn every_way_a_process_stops_reads_back_as_itself() {
    use rust_mutants::execute::Stopped;
    use rust_mutants::runner::ProcessExit;
    for stopped in [
        Stopped::NotStarted {
            cause: StartFailure::Missing,
        },
        Stopped::NotStarted {
            cause: StartFailure::Other {
                detail: "Operation not supported (os error 45)".to_owned(),
            },
        },
        Stopped::Exited {
            exit: ProcessExit::Code(3),
        },
        Stopped::Exited {
            exit: ProcessExit::Signal(9),
        },
        Stopped::Exited {
            exit: ProcessExit::Unknown,
        },
        Stopped::TimedOut { raised: Some(4) },
        Stopped::Stalled { raised: None },
        Stopped::Cancelled { started: true },
        Stopped::WaitFailed,
        Stopped::Answered,
    ] {
        let written = serde_json::to_string(&stopped).expect("a stop serializes");
        let read: Stopped = njutest_devkit::strictjson::decode_str(&written)
            .unwrap_or_else(|error| panic!("{written} does not read back: {error}"));
        assert_eq!(
            read, stopped,
            "a stop the engine can reach is one a recording can hold and a reader can read: {written}"
        );
    }
}

#[test]
fn a_harness_that_never_started_says_why() {
    let result = rust_mutants::execute::exec(
        &ExecRequest::new(&target()),
        &Context {
            leaders: None,
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: None,
            beside: None,
            touch: None,
            steps: None,
            crash: None,
            profile: None,
        },
        &rust_mutants::runner::Cancel::new(),
        &rust_mutants::trace::Recorder::disabled(),
    );
    assert_eq!(
        result.stopped,
        Stopped::NotStarted {
            cause: StartFailure::Missing
        },
        "a test binary that is not there is named as missing, which is what a row that says \
         only `exit -1` left a person to guess"
    );
}

#[test]
fn a_harness_that_named_a_failure_is_heard_even_where_its_process_exited_zero() {
    let failed = vec!["noticed_the_mutation".to_owned()];
    assert_eq!(
        outcome_of(&result(0), Some(green()), (true, &failed)),
        Outcome::Killed,
        "a test the harness said failed noticed the mutation, as it does before a stop, a clock \
         or a signal; an exit status of zero does not take that back, and reading it as a \
         survivor would claim no test noticed what one said it did"
    );
    let failing = Summary {
        ok: false,
        passed: 1,
        failed: 1,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
    };
    assert_eq!(
        outcome_of(&result(0), Some(failing), (true, &[])),
        Outcome::Inconclusive,
        "a summary that counts a failure it names nowhere, from a process that exited zero, \
         contradicts itself, and a contradiction is no survivor"
    );
}

/// The specification of the decision, which the engine's `outcome_of` is one implementation of.
const VERDICTS: &str = include_str!("../../../docs/engine/verdicts.md");

/// Every row of the page's decision table, in the order it is read: six cells of what the decision reads and the verdict.
fn verdict_table() -> Vec<[String; 7]> {
    let mut rows = Vec::new();
    let mut malformed = Vec::new();
    let mut inside = false;
    for line in VERDICTS.lines() {
        if line.starts_with("| stopped |") {
            inside = true;
            continue;
        }
        if !inside || line.starts_with("| ---") {
            continue;
        }
        if !line.starts_with('|') {
            break;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect();
        match <[String; 7]>::try_from(cells) {
            Ok(row) => rows.push(row),
            Err(cells) => malformed.push(cells),
        }
    }
    assert!(
        malformed.is_empty(),
        "every row of the decision table has seven cells: {malformed:?}"
    );
    rows
}

/// The first row of `table` whose cells all match `words`, by its place, and the verdict it gives.
fn first_row<'t>(table: &'t [[String; 7]], words: [&str; 6]) -> Option<(usize, &'t str)> {
    table.iter().enumerate().find_map(|(at, row)| {
        let matched = row
            .iter()
            .zip(words)
            .all(|(cell, word)| cell == "*" || cell.split(", ").any(|one| one == word));
        matched.then(|| (at, row[6].as_str()))
    })
}

/// A summary as the page names it.
const fn summary_word(summary: Option<Summary>) -> &'static str {
    match summary {
        None => "none",
        Some(said) if said.passed == 0 && said.failed == 0 => "ran-nothing",
        Some(said) if said.ok && said.failed == 0 => "clean",
        Some(_) => "failing",
    }
}

/// Every way a process can stop that the decision tells apart, as the page names it and its exit.
fn every_stop() -> Vec<(&'static str, &'static str, Stopped)> {
    let mut stops = vec![
        (
            "not-started",
            "*",
            Stopped::NotStarted {
                cause: StartFailure::Missing,
            },
        ),
        ("wait-failed", "*", Stopped::WaitFailed),
        (
            "step-protocol-failed",
            "*",
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::Publication {},
            },
        ),
        (
            "step-limit-reached",
            "*",
            Stopped::StepLimitReached {
                notice: step_notice(),
            },
        ),
        ("timed-out", "*", Stopped::TimedOut { raised: None }),
        ("timed-out", "*", Stopped::TimedOut { raised: Some(3) }),
        ("stalled", "*", Stopped::Stalled { raised: None }),
        ("cancelled", "*", Stopped::Cancelled { started: true }),
        ("cancelled", "*", Stopped::Cancelled { started: false }),
        ("answered", "*", Stopped::Answered),
    ];
    let mut exits = vec![
        ("code-zero", ProcessExit::Code(0)),
        ("code-other", ProcessExit::Code(1)),
        ("code-other", ProcessExit::Code(101)),
        ("unknown", ProcessExit::Unknown),
        ("outside-signal", ProcessExit::Signal(9)),
        ("outside-signal", ProcessExit::Signal(15)),
    ];
    if cfg!(unix) {
        exits.push(("self-signal", ProcessExit::Signal(6)));
        exits.push(("self-signal", ProcessExit::Signal(11)));
    }
    for (word, exit) in exits {
        stops.push(("exited", word, Stopped::Exited { exit }));
    }
    stops
}

/// A summary line with these counts and nothing ignored or measured.
const fn summary_line(ok: bool, passed: u32, failed: u32, filtered_out: u32) -> Summary {
    Summary {
        ok,
        passed,
        failed,
        ignored: 0,
        measured: 0,
        filtered_out,
    }
}

/// Every summary line the decision tells apart, twice over where two lines read the same.
const fn every_summary() -> [Option<Summary>; 7] {
    [
        None,
        Some(green()),
        Some(summary_line(true, 0, 0, 3)),
        Some(summary_line(false, 0, 0, 0)),
        Some(summary_line(false, 1, 1, 0)),
        Some(summary_line(true, 1, 1, 0)),
        Some(summary_line(false, 2, 0, 0)),
    ]
}

/// One combination of everything the decision reads: the page's words for it, and the engine's own arguments.
struct Reading {
    words: [&'static str; 6],
    observed: Observation,
    summary: Option<Summary>,
    harness: bool,
    named: bool,
}

/// Every combination of everything the decision reads, a harness that is not libtest naming nothing and printing no summary.
fn every_reading() -> Vec<Reading> {
    let mut readings = Vec::new();
    for (stop, exit, stopped) in every_stop() {
        for stale in [false, true] {
            for (harness, named) in [(false, false), (true, false), (true, true)] {
                let summaries: &[Option<Summary>] =
                    if harness { &every_summary() } else { &[None] };
                for summary in summaries {
                    readings.push(Reading {
                        words: [
                            stop,
                            exit,
                            if harness { "yes" } else { "no" },
                            if named { "yes" } else { "no" },
                            summary_word(*summary),
                            if stale { "yes" } else { "no" },
                        ],
                        observed: Observation {
                            stopped: stopped.clone(),
                            stale_catalog: stale,
                        },
                        summary: *summary,
                        harness,
                        named,
                    });
                }
            }
        }
    }
    readings
}

#[test]
fn the_decision_is_the_page_s_table_for_everything_it_reads() {
    let table = verdict_table();
    assert!(!table.is_empty(), "docs/engine/verdicts.md holds a table");
    let failed = ["noticed_it".to_owned()];
    let mut used = vec![false; table.len()];
    let mut differ = Vec::new();
    let mut undecided = Vec::new();
    for reading in every_reading() {
        let heard: &[String] = if reading.named { &failed } else { &[] };
        let engine =
            outcome_of(&reading.observed, reading.summary, (reading.harness, heard)).name();
        let Some((at, verdict)) = first_row(&table, reading.words) else {
            undecided.push(reading.words.join(" | "));
            continue;
        };
        if let Some(one) = used.get_mut(at) {
            *one = true;
        }
        if verdict != engine {
            differ.push(format!(
                "{}: the page says {verdict}, the engine {engine}",
                reading.words.join(" | ")
            ));
        }
    }
    differ.sort();
    differ.dedup();
    assert!(
        undecided.is_empty(),
        "no row of the page decides these: {undecided:#?}"
    );
    assert!(
        differ.is_empty(),
        "the engine decides otherwise than the page: {differ:#?}"
    );
    let produced = produced_words();
    let dead: Vec<&[String; 7]> = table
        .iter()
        .zip(&used)
        .filter(|(row, used)| !**used && reachable_here(row, &produced))
        .map(|(row, _)| row)
        .collect();
    assert!(
        dead.is_empty(),
        "a row no combination reaches says nothing: {dead:#?}"
    );
}

/// Every word each column of the page can read on this platform, which is what a row can be reached through here.
fn produced_words() -> [std::collections::BTreeSet<&'static str>; 6] {
    let mut produced: [std::collections::BTreeSet<&'static str>; 6] = Default::default();
    for reading in every_reading() {
        for (column, word) in produced.iter_mut().zip(reading.words) {
            column.insert(word);
        }
    }
    produced
}

/// Whether every word `row` asks for is one this platform produces: a `self-signal` exists only where a process can end of a signal it raised.
fn reachable_here(
    row: &[String; 7],
    produced: &[std::collections::BTreeSet<&'static str>; 6],
) -> bool {
    row.iter()
        .zip(produced)
        .all(|(cell, words)| cell == "*" || cell.split(", ").any(|one| words.contains(one)))
}

#[cfg(unix)]
#[test]
fn the_signals_a_process_raises_by_itself_are_the_page_s() {
    let marked = VERDICTS
        .split("<!-- self-raised-signals -->")
        .nth(1)
        .and_then(|rest| rest.split("<!-- /self-raised-signals -->").next());
    assert!(
        marked.is_some(),
        "the page marks its list of self-raised signals"
    );
    let listed: Vec<&str> = marked
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- `"))
        .filter_map(|line| line.strip_suffix('`'))
        .collect();
    let known = [
        ("SIGABRT", rustix::process::Signal::ABORT),
        ("SIGSEGV", rustix::process::Signal::SEGV),
        ("SIGBUS", rustix::process::Signal::BUS),
        ("SIGILL", rustix::process::Signal::ILL),
        ("SIGFPE", rustix::process::Signal::FPE),
        ("SIGTRAP", rustix::process::Signal::TRAP),
        ("SIGSYS", rustix::process::Signal::SYS),
    ];
    let mut expected = Vec::new();
    for name in &listed {
        let number = known.iter().find(|(said, _)| said == name);
        assert!(
            number.is_some(),
            "{name} is a signal this test knows the number of"
        );
        if let Some((_, signal)) = number {
            expected.push(signal.as_raw());
        }
    }
    expected.sort_unstable();
    let raised: Vec<i32> = (1..=64)
        .filter(|signal| ProcessExit::Signal(*signal).raised_by_itself())
        .collect();
    assert_eq!(
        raised, expected,
        "the engine's self-raised signals are the page's: {listed:?}"
    );
}
