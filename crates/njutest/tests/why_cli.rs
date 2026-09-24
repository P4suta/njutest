// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest why seam` follows one typed seam claim through the command boundary.
//!
//! Every test here needs a published report, and `Store::keep` answers `NJ6004` on Windows because publication is rooted at a POSIX directory capability. `docs/limitations.md` says so; these say it by not existing there.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a synthetic run that cannot be written is a test setup failure"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use njutest::cli::Environment;
use njutest::report::{RunKind, SeamDecision};
use njutest::trace::{
    Clock, DirSink, Read, Recorder, Sink, StartRecord, WireExchangeRecord, WireExecRecord,
};
use njutest::wire::rule::Rule;
use rust_mutants::runner::Cancel;

const RUN: &str = "20260920t000000z-why-seam";
const FAULT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test starts no subprocess"),
        vars: njutest_devkit::paths::environment_for_a_run(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn named_run(root: &Path) {
    let directory = root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(RUN);
    std::fs::create_dir_all(directory).expect("the synthetic run directory");
}

fn recorded(root: &Path) {
    named_run(root);
    let recording = root.join(".njutest/trace").join(RUN);
    let trace = Recorder::new(
        Sink::required_with_ring(DirSink::create(&recording).expect("the recording")),
        Clock::stepping(
            Timestamp::from_second(1_800_000_000).expect("a timestamp"),
            Duration::from_millis(1),
        ),
        StartRecord::of(RUN, RunKind::Full, njutest::config::Contract::StandardV1),
    );
    trace.wire_exchange(WireExchangeRecord {
        capability: "payments".to_owned(),
        seq: 7,
        during: Some("workspace/test/payments".to_owned()),
        duration_ms: 3,
        read: Read::Http {
            method: "POST".to_owned(),
            path: "/charge".to_owned(),
            status: 200,
        },
        request_bytes: 41,
        response_bytes: 18,
    });
    trace.wire_exec(WireExecRecord {
        fault: FAULT.to_owned(),
        capability: "payments".to_owned(),
        seq: 7,
        rule: Rule::StatusServerError,
        decision: SeamDecision::Tests {
            noticed_by: "workspace/test/payments".to_owned(),
        },
    });
    trace.run_end("ASSURED", None, None).expect("trace closes");
}

fn ask(root: &Path, id: &str) -> (u8, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "why", "--run", RUN, "seam", id]
            .into_iter()
            .map(OsString::from),
        &environment(root),
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        njutest_devkit::process::strict_utf8(&stdout).into_owned(),
        njutest_devkit::process::strict_utf8(&stderr).into_owned(),
    )
}

#[test]
fn why_seam_reads_the_exchange_and_decision_as_one_chain() {
    let root = njutest_devkit::paths::Project::fresh();
    recorded(root.path());
    let (code, stdout, stderr) = ask(root.path(), FAULT);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    for part in [
        FAULT,
        "observed payments #7",
        "POST /charge -> 200",
        "put status-server-error",
        "workspace/test/payments noticed it",
    ] {
        assert!(stdout.contains(part), "{part:?} is absent from:\n{stdout}");
    }
    assert!(
        !stdout.contains("routed") && !stdout.contains("asked"),
        "a seam claim must not be rendered as a mutation chain:\n{stdout}"
    );
}

#[test]
fn an_unknown_seam_is_distinct_from_a_run_that_kept_no_recording() {
    let root = njutest_devkit::paths::Project::fresh();
    recorded(root.path());
    let unknown = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let (code, stdout, stderr) = ask(root.path(), unknown);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(
        stdout.contains("the recording does not hold this seam question")
            && stdout.contains("1 other seam question"),
        "{stdout}"
    );

    let root = njutest_devkit::paths::Project::fresh();
    named_run(root.path());
    let (code, stdout, stderr) = ask(root.path(), FAULT);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(
        stdout.contains("this run kept no recording"),
        "an absent recording must not be called a recording that omitted this id: {stdout}"
    );
}

#[test]
fn an_unreadable_recording_is_an_error_not_an_absent_recording() {
    let root = njutest_devkit::paths::Project::fresh();
    named_run(root.path());
    let recording = root.path().join(".njutest/trace").join(RUN);
    std::fs::create_dir_all(&recording).expect("the recording directory");
    std::fs::write(
        recording.join(njutest::trace::FILE_NAME),
        b"not a trace event\n",
    )
    .expect("the malformed recording");

    let (code, stdout, stderr) = ask(root.path(), FAULT);
    assert_eq!(code, njutest::cli::EXIT_ERROR, "{stdout}{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("NJ6005"), "{stderr}");
    assert!(!stderr.contains("kept no recording"), "{stderr}");
}
