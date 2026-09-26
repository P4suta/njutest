// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `deep-v1` soundness phase: what interpreting the suite establishes, and what it refuses to call a pass.

#![cfg_attr(
    unix,
    expect(
        clippy::expect_used,
        reason = "the helpers that build one recording and one request are not themselves tests, and a value out of range is a setup failure to report by panicking"
    )
)]
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use njutest::assure::deep::{Absent, Interpreted, Interpreting, interpret};
use njutest::error::RunnerError;
use njutest::report::FindingKind;
use njutest::trace::Recorder;
use njutest::watch::Watch;
use rust_mutants::runner::Cancel;

/// The cargo every test here drives, read rather than written: see the script's own note.
fn cargo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-cargo.sh")
}

/// What that cargo is told to say, and how it is told to end.
fn saying(said: &str, code: i32) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(name, _)| njutest_devkit::paths::same_name(name, std::ffi::OsStr::new("PATH")))
        .collect();
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_SAYS"),
        std::ffi::OsString::from(said),
    ));
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_CODE"),
        std::ffi::OsString::from(code.to_string()),
    ));
    env
}

/// What Miri prints for a suite of one binary whose one test passed.
const PASSED: &str = "     Running unittests src/lib.rs (x)\n\nrunning 1 test\ntest t ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

/// What Miri prints for a suite of one binary whose one test failed.
const FAILED_RUN: &str = "     Running unittests src/lib.rs (x)\n\nrunning 1 test\ntest t ... FAILED\n\nfailures:\n\n---- t stdout ----\nboom\n\nfailures:\n    t\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

fn recording() -> Recorder {
    Recorder::new(
        njutest::trace::Sink::Memory(njutest::trace::MemorySink::unbounded()),
        njutest::trace::Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            Duration::from_secs(1),
        ),
        njutest::trace::StartRecord::of(
            "20260909T000000Z-000001",
            njutest::report::RunKind::Full,
            njutest::config::Contract::DeepV1,
        ),
    )
}

fn interpreted(said: &str, code: i32, dir: &Path) -> Result<Interpreted, RunnerError> {
    interpreted_when_absent(said, code, dir, Absent::Refused)
}

fn interpreted_when_absent(
    said: &str,
    code: i32,
    dir: &Path,
    absent: Absent,
) -> Result<Interpreted, RunnerError> {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    interpret(
        &Interpreting {
            root: dir,
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env: saying(said, code),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
            absent,
        },
        Watch::new(&cancel, &trace),
    )
}

#[test]
fn a_suite_the_interpreter_passes_is_one_the_run_says_was_interpreted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = PASSED;
    let done = interpreted(said, 0, dir.path()).expect("ran");
    assert_eq!(
        done,
        Interpreted {
            executed: true,
            findings: Vec::new(),
            limitations: Vec::new(),
        }
    );
}

#[test]
fn undefined_behaviour_is_a_defect_and_not_a_gap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: Undefined Behavior: trying to retag from <1234> for Unique permission";
    let done = interpreted(said, 1, dir.path()).expect("ran");
    assert!(done.executed);
    let finding = done.findings.first().expect("a finding");
    assert_eq!(finding.kind, FindingKind::UndefinedBehaviour);
    assert!(finding.kind.is_defect());
    assert!(finding.detail.contains("Undefined Behavior"), "{finding:?}");
    assert!(done.limitations.is_empty(), "{done:?}");
}

#[test]
fn what_the_interpreter_will_not_interpret_is_stated_and_never_read_as_a_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: unsupported operation: can't call foreign function `getrandom`";
    let done = interpreted(said, 1, dir.path()).expect("ran");
    assert!(done.executed);
    assert_eq!(
        done.limitations.first().map(|one| one.name.clone()),
        Some("miri-unsupported".to_owned())
    );
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::NotMeasured),
        "a suite that was not interpreted whole is not one that was found sound"
    );
}

#[test]
fn a_test_that_fails_under_the_interpreter_is_a_failing_test() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = FAILED_RUN;
    let done = interpreted(said, 101, dir.path()).expect("ran");
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::FailingTest)
    );
}

#[test]
fn a_toolchain_with_no_interpreter_cannot_answer_a_contract_that_promises_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: 'cargo-miri' is not installed for the toolchain 'nightly'";
    let refused = interpreted(said, 1, dir.path()).expect_err("no interpreter");
    assert_eq!(refused.code().code, "NJ7001", "{refused}");
    assert!(matches!(refused, RunnerError::MiriMissing { .. }));
}

#[test]
fn a_machine_with_no_nightly_at_all_is_a_toolchain_with_no_interpreter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: toolchain 'nightly-x86_64-unknown-linux-gnu' is not installed\n\
                help: run `rustup toolchain install nightly-x86_64-unknown-linux-gnu` to install it";
    let refused = interpreted(said, 1, dir.path()).expect_err("no interpreter");
    assert_eq!(
        refused.code().code,
        "NJ7001",
        "a toolchain that is not there and a component that is not there are one thing \
         to the run: there is nothing to interpret with. Reading only the second leaves \
         the first as a suite that failed, and a contract promising interpretation then \
         answers without having interpreted anything: {refused}"
    );
    assert!(matches!(refused, RunnerError::MiriMissing { .. }));
}

#[test]
fn a_cargo_that_is_not_there_is_a_toolchain_with_no_interpreter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let refused = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&dir.path().join("nothing")),
            env: saying("", 0),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect_err("no interpreter");
    assert_eq!(refused.code().code, "NJ7001", "{refused}");
}

#[test]
fn what_a_run_promised_cargo_it_would_not_do_is_said_to_this_cargo_too() {
    let dir = tempfile::tempdir().expect("a directory");
    let cancel = Cancel::new();
    let trace = recording();
    let cargo = cargo();

    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env: saying(PASSED, 0),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture interpreter output is valid UTF-8");
    assert!(done.executed, "the interpreter command completed");

    let exec = trace
        .events()
        .iter()
        .find_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .exec()
                .cloned()
        })
        .expect("the command it started");
    assert_eq!(
        exec.argv,
        vec![
            cargo.display().to_string(),
            "+nightly".to_owned(),
            "miri".to_owned(),
            "test".to_owned(),
            "--workspace".to_owned(),
            "--offline".to_owned(),
            "--locked".to_owned(),
        ],
        "the whole of what a run asks of the interpreter, in the order it asks it: the \
         toolchain it names, that it is Miri and not the tests, what it interprets, and \
         the two promises about cargo that a run makes to every command it starts"
    );
    assert_eq!(
        exec.dir.as_deref(),
        Some(dir.path().display().to_string().as_str()),
        "and it runs in the workspace, because a rust-toolchain file there is what says \
         which nightly answers"
    );
    assert_eq!(exec.timeout_ms, Some(30_000));
}

#[test]
fn a_run_that_named_packages_asks_the_interpreter_for_those_and_not_the_workspace() {
    let dir = tempfile::tempdir().expect("a directory");
    let cancel = Cancel::new();
    let trace = recording();
    let cargo = cargo();
    let packages = ["core".to_owned(), "app".to_owned()];

    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env: saying(PASSED, 0),
            packages: &packages,
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: false,
            locked: false,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture interpreter output is valid UTF-8");
    assert!(done.executed, "the interpreter command completed");

    let argv: Vec<String> = trace
        .events()
        .iter()
        .find_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .exec()
                .map(|exec| exec.argv.clone())
        })
        .expect("the command it started");
    assert_eq!(
        argv,
        vec![
            cargo.display().to_string(),
            "+nightly".to_owned(),
            "miri".to_owned(),
            "test".to_owned(),
            "--package".to_owned(),
            "core".to_owned(),
            "--package".to_owned(),
            "app".to_owned(),
        ],
        "a run narrowed to packages interprets those and says so package by package: \
         asking for the workspace would interpret code this run was not asked about and \
         report what it found there"
    );
}

#[test]
fn the_flags_a_configuration_gives_the_interpreter_are_the_ones_it_ran_with() {
    let dir = tempfile::tempdir().expect("a directory");
    let seen = dir.path().join("environment");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = saying(PASSED, 0);
    env.push((
        std::ffi::OsString::from("MIRIFLAGS"),
        std::ffi::OsString::from("-Zmiri-from-the-outside"),
    ));
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_ENV_OUT"),
        std::ffi::OsString::from(seen.display().to_string()),
    ));
    let flags = [
        "-Zmiri-strict-provenance".to_owned(),
        "-Zmiri-symbolic-alignment-check".to_owned(),
    ];

    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env,
            packages: &[],
            flags: &flags,
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture interpreter output is valid UTF-8");
    assert!(done.executed, "the interpreter command completed");

    let said = std::fs::read_to_string(&seen).expect("what the cargo it started saw");
    assert!(
        said.contains("MIRIFLAGS=-Zmiri-strict-provenance -Zmiri-symbolic-alignment-check\n"),
        "the interpreter is told what to check by this one variable, so a run that \
         composed the flags and did not hand them over interprets under the defaults and \
         reports what those found: {said}"
    );
    assert!(
        !said.contains("-Zmiri-from-the-outside"),
        "and what the process already carried is replaced rather than added to, because \
         two values of one variable is one value and which one it is would depend on the \
         order a list happened to be in: {said}"
    );
}

#[test]
fn an_interpreter_left_to_run_as_it_was_started_keeps_the_variable_it_was_given() {
    let dir = tempfile::tempdir().expect("a directory");
    let seen = dir.path().join("environment");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = saying(PASSED, 0);
    env.push((
        std::ffi::OsString::from("MIRIFLAGS"),
        std::ffi::OsString::from("-Zmiri-from-the-outside"),
    ));
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_ENV_OUT"),
        std::ffi::OsString::from(seen.display().to_string()),
    ));

    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env,
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture interpreter output is valid UTF-8");
    assert!(done.executed, "the interpreter command completed");

    let said = std::fs::read_to_string(&seen).expect("what the cargo it started saw");
    assert!(
        said.contains("MIRIFLAGS=-Zmiri-from-the-outside\n"),
        "a configuration that asked for no flags of its own is not a configuration that \
         asked for none at all: taking away what the run was started with would quietly \
         interpret under different rules than the person who set it expects: {said}"
    );
}

#[test]
fn an_interpreter_that_runs_out_of_time_has_interpreted_nothing() {
    let dir = tempfile::tempdir().expect("a directory");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = saying(PASSED, 0);
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_SLEEP"),
        std::ffi::OsString::from("5"),
    ));

    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env,
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_millis(200)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("a run that was stopped is not a toolchain without an interpreter");

    assert!(
        !done.executed,
        "an interpretation that was stopped is not an interpretation: a contract that \
         promises the suite is interpreted would be answered by one that got part of \
         the way through and was cut off: {done:?}"
    );
    assert!(
        done.limitations
            .iter()
            .any(|one| one.detail.contains("ran out of time")
                && one.detail.contains("not interpreted whole")),
        "and it says which of the ways it could come back empty this was, because more \
         time and a suite the interpreter cannot follow are different things to do: \
         {done:?}"
    );
    assert!(
        done.findings.is_empty(),
        "and it claims nothing about what it did not finish reading: {done:?}"
    );
}

#[test]
fn what_the_interpreter_established_is_said_in_words_a_person_can_act_on() {
    let dir = tempfile::tempdir().expect("a directory");

    let found = interpreted(
        "test demo ... error: Undefined Behavior: attempting a read access",
        1,
        dir.path(),
    )
    .expect("an interpreter that ran");
    assert!(
        found.findings.iter().any(
            |one| one.subject == "soundness" && one.detail.contains("attempting a read access")
        ),
        "what the interpreter said is quoted rather than summarised, because the line \
         it printed is where the reader goes next: {found:?}"
    );

    let refused = interpreted("error: unsupported operation: `mmap`", 1, dir.path())
        .expect("an interpreter that ran");
    assert!(
        refused.limitations.iter().any(|one| one
            .detail
            .contains("could not interpret the suite whole")
            && one.detail.contains("`mmap`")),
        "and what it would not follow says what it was: {refused:?}"
    );
    assert!(
        refused.findings.iter().any(|one| one.subject == "soundness"
            && one
                .detail
                .contains("nothing is claimed about the unsafe it holds")),
        "and the finding says what is therefore not established, which is the whole \
         difference between a gap and a pass: {refused:?}"
    );

    let failing = interpreted(FAILED_RUN, 101, dir.path()).expect("an interpreter that ran");
    assert!(
        failing.findings.iter().any(|one| one.subject == "soundness"
            && one.detail == "a test fails under the interpreter that passes without it"),
        "and a suite that fails only under the interpreter says that it is the \
         interpreter that makes the difference: {failing:?}"
    );
}

#[test]
fn a_toolchain_without_an_interpreter_says_what_it_was_told_rather_than_what_it_assumed() {
    let dir = tempfile::tempdir().expect("a directory");
    let refused = interpreted(
        "warning: something else entirely\nerror: no such subcommand: `miri`",
        1,
        dir.path(),
    )
    .expect_err("no interpreter");
    assert!(
        refused.to_string().contains("no such subcommand"),
        "the line the toolchain printed is what a person acts on, and the first line of \
         whatever came back is not that line: {refused}"
    );

    let quiet = interpreted(
        "error: 'cargo-miri' is not installed for the toolchain 'nightly'",
        1,
        dir.path(),
    )
    .expect_err("no interpreter");
    assert!(
        quiet.to_string().contains("miri"),
        "and a toolchain that said so without the word this looks for still leaves \
         something to read rather than an empty refusal: {quiet}"
    );
}

#[test]
fn an_interpreter_that_ran_out_of_time_interpreted_nothing_whole() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = saying(PASSED, 0);
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_SLEEP"),
        std::ffi::OsString::from("5"),
    ));
    let done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: rust_mutants::cargo::Selecting::named(&cargo),
            env,
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_millis(300)),
            offline: true,
            locked: true,
            absent: Absent::Refused,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the interpreter was started");

    assert!(
        !done.executed,
        "an interpretation that ran out of time is not one that happened, and a contract \
         that promises soundness cannot be answered by a suite nobody finished \
         interpreting: {done:?}"
    );
    assert_eq!(
        done.limitations
            .first()
            .map(|one| one.name.clone())
            .as_deref(),
        Some(njutest::limitation::MIRI_TIMED_OUT),
        "and the reason is the time rather than anything about the code, which is the \
         difference between a run to give more time to and a defect to go and fix: {done:?}"
    );
    assert!(
        done.findings.is_empty(),
        "a suite the interpreter never finished says nothing about the program, and a \
         finding here would be one nobody can act on: {done:?}"
    );
}

#[test]
fn a_contract_that_asks_every_dimension_names_a_missing_interpreter_as_a_hole() {
    let dir = tempfile::tempdir().expect("tempdir");
    for said in [
        "error: 'cargo-miri' is not installed for the toolchain 'nightly'",
        "error: toolchain 'nightly-aarch64-apple-darwin' is not installed",
    ] {
        let done = interpreted_when_absent(said, 1, dir.path(), Absent::Hole)
            .expect("a machine with no interpreter is a hole under this contract, not an error");
        assert!(!done.executed, "{done:?}");
        assert_eq!(
            done.limitations.first().map(|one| one.name.clone()),
            Some("miri-unavailable".to_owned()),
            "{done:?}"
        );
        assert_eq!(
            done.findings.first().map(|one| one.kind),
            Some(FindingKind::NotMeasured),
            "a suite nothing interpreted is not one found sound, so the run is not assured: {done:?}"
        );
    }
}

#[test]
fn an_interpreter_that_ran_no_test_found_nothing_to_fail_and_interpreted_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let setup_failed = "WARNING: Ignoring `RUSTC_WRAPPER` environment variable\n\
                        thread 'main' panicked at src/tools/miri/cargo-miri/src/util.rs:132:9:\n\
                        failed to run `cd \"/gone\" && env ...`";
    let done = interpreted(setup_failed, 101, dir.path()).expect("ran");
    assert!(!done.executed, "{done:?}");
    assert!(
        !done
            .findings
            .iter()
            .any(|finding| finding.kind == FindingKind::FailingTest),
        "no test ran, so no test failed: an exit status with no test result under it is the \
         interpreter's own trouble, and a verdict of DEFECT on it blames the suite: {done:?}"
    );
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::NotMeasured),
        "{done:?}"
    );
    assert_eq!(
        done.limitations.first().map(|one| one.name.clone()),
        Some("miri-ran-no-test".to_owned()),
        "{done:?}"
    );
    assert!(
        done.limitations
            .first()
            .is_some_and(|one| one.detail.contains("failed to run `cd")),
        "a hole says what the interpreter said last, so a reader can see why no test ran: \
         {done:?}"
    );
    let said_nothing = interpreted("   Compiling demo v0.1.0\n", 0, dir.path()).expect("ran");
    assert!(
        !said_nothing.executed,
        "a run that says it passed and ran no test interpreted nothing: {said_nothing:?}"
    );
    assert_eq!(
        said_nothing.findings.first().map(|one| one.kind),
        Some(FindingKind::NotMeasured),
        "{said_nothing:?}"
    );
}

#[test]
fn the_phase_comes_to_the_verdict_the_published_contract_gives_every_case() {
    let contract: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/miri-output.json"),
        )
        .expect("the published contract"),
    )
    .expect("the contract is JSON");
    let cases = contract
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .expect("the contract's cases");
    assert!(cases.len() >= 10, "the corpus is read: {}", cases.len());
    let dir = tempfile::tempdir().expect("tempdir");
    for case in cases {
        let field = |key: &str| case.get(key).and_then(serde_json::Value::as_str);
        let name = field("name").expect("a case is named");
        let output = field("output").expect("a case has output");
        let status = i32::try_from(
            case.get("status")
                .and_then(serde_json::Value::as_i64)
                .expect("a case has a status"),
        )
        .expect("a status an exit code can be");
        let done = interpreted(output, status, dir.path()).expect("the interpreter is there");
        let kinds: Vec<FindingKind> = done.findings.iter().map(|one| one.kind).collect();
        let limitations: Vec<&str> = done
            .limitations
            .iter()
            .map(|one| one.name.as_str())
            .collect();
        let came = match (done.executed, kinds.as_slice(), limitations.as_slice()) {
            (true, [], []) => "passed",
            (true, [FindingKind::FailingTest], []) => "failed",
            (true, [FindingKind::UndefinedBehaviour], []) => "undefined",
            (true, [FindingKind::NotMeasured], ["miri-unsupported"]) => "unsupported",
            (false, [FindingKind::NotMeasured], ["miri-ran-no-test"]) => "ran-no-test",
            other => panic!("{name}: an outcome the contract has no name for: {other:?}"),
        };
        assert_eq!(
            Some(came),
            field("came"),
            "{name}: the runner and the audit read one published contract of what Miri writes, \
             and this case says what it comes to"
        );
    }
}
