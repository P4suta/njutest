// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The answers earlier runs reached: what is stored, what is refused, what is read back, and what happens when two runs want the same one at once.

use std::time::Duration;

use jiff::Timestamp;
use mjutest_cli::cache::lock::{self, LeaseError};
use mjutest_cli::cache::store::{CacheError, Store};
use mjutest_cli::report::{Provenance, Report, RunKind, TargetRecord, TargetStatus, Verdict};
use rust_mutants::runner::Cancel;

fn report(run_id: &str, identity: &str) -> Report {
    let mut report = Report::new(
        run_id,
        RunKind::Full,
        mjutest_cli::config::Contract::StandardV1,
    );
    report.provenance = Provenance {
        identity: identity.to_owned(),
        cached: false,
        source_run_id: None,
    };
    "demo".clone_into(&mut report.repository.root_name);
    report.repository.workspace_digest = "a".repeat(64);
    report.repository.configuration_digest = "b".repeat(64);
    "rustc 1.98.0".clone_into(&mut report.toolchain.rustc);
    report
        .limitations
        .push(mjutest_cli::report::Limitation::new(
            "git-metadata-unavailable",
            "the tree a test builds is not a git repository",
        ));
    report.verdict = Verdict::Assured;
    report.accounting.targets.selected = 1;
    report.accounting.targets.passed = 1;
    report.targets.push(TargetRecord {
        id: "demo/lib/demo".to_owned(),
        package: "demo".to_owned(),
        name: "demo".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    report
}

fn store(root: &std::path::Path) -> Store {
    Store::new(root, 1024 * 1024, Duration::from_hours(24))
}

#[test]
fn what_a_run_established_is_read_back_by_the_next_run_of_the_same_inputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let identity = "c".repeat(64);
    assert!(
        store
            .get(&identity)
            .expect("a miss is not a failure")
            .is_none(),
        "nothing was stored yet"
    );

    let written = store.put(&report("run-1", &identity)).expect("stored");
    assert!(written.is_file());
    let read = store
        .get(&identity)
        .expect("stored")
        .expect("what was stored is there");
    assert_eq!(read.run_id, "run-1");
    assert_eq!(read.verdict, Verdict::Assured);
    assert!(
        store.get(&"d".repeat(64)).expect("a miss").is_none(),
        "a different identity is a different question"
    );
}

#[test]
fn an_answer_that_is_not_the_answer_it_claims_to_be_is_never_quietly_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let identity = "c".repeat(64);
    store.put(&report("run-1", &identity)).expect("stored");

    std::fs::write(store.entry(&identity), "{ not a report").expect("write");
    let error = store
        .get(&identity)
        .expect_err("a document that does not parse");
    assert!(matches!(error, CacheError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("MJ8004"), "{error}");

    let mut misfiled = report("run-1", &"e".repeat(64));
    misfiled.provenance.identity = "e".repeat(64);
    let text = mjutest_cli::report::json::render(&misfiled).expect("render");
    std::fs::write(store.entry(&identity), text).expect("write");
    let error = store
        .get(&identity)
        .expect_err("an answer filed under the wrong question");
    assert!(error.to_string().contains(&identity), "{error}");

    let mut unsound = report("run-1", &identity);
    unsound.accounting.targets.selected = 9;
    let text = mjutest_cli::report::json::render(&unsound).expect("render");
    std::fs::write(store.entry(&identity), text).expect("write");
    let error = store
        .get(&identity)
        .expect_err("an answer no reader could check");
    assert!(matches!(error, CacheError::Corrupt { .. }), "{error}");
}

#[test]
fn a_report_that_answers_for_no_inputs_or_was_itself_read_back_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());

    let mut nameless = report("run-1", "");
    nameless.provenance.identity = String::new();
    let error = store.put(&nameless).expect_err("no identity");
    assert!(matches!(error, CacheError::Refused { .. }), "{error}");

    let mut copied = report("run-2", &"c".repeat(64));
    copied.provenance.cached = true;
    copied.provenance.source_run_id = Some("run-1".to_owned());
    let error = store.put(&copied).expect_err("already stored elsewhere");
    assert!(
        error.to_string().contains("read back"),
        "a chain of copies is not a chain of evidence: {error}"
    );

    let mut unsound = report("run-3", &"c".repeat(64));
    unsound.accounting.targets.selected = 9;
    let error = store
        .put(&unsound)
        .expect_err("not one a reader could check");
    assert!(matches!(error, CacheError::Refused { .. }), "{error}");
    assert!(
        store.get(&"c".repeat(64)).expect("a miss").is_none(),
        "nothing refused was written"
    );
}

#[test]
fn the_store_says_what_it_holds_and_collects_what_it_should_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    assert_eq!(store.status().expect("an empty store").entries, 0);

    for index in 0..4u8 {
        let identity = format!("{index:064x}");
        store
            .put(&report(&format!("run-{index}"), &identity))
            .expect("stored");
    }
    let status = store.status().expect("a store");
    assert_eq!(status.entries, 4);
    assert!(status.bytes > 0);

    let bounded = Store::new(dir.path(), 1, Duration::from_hours(24));
    let collected = bounded.collect(Timestamp::now()).expect("collected");
    assert!(collected.expired.is_empty(), "nothing was old");
    assert_eq!(
        collected.evicted.len(),
        4,
        "a store smaller than one entry keeps none of them"
    );
    assert_eq!(store.status().expect("a store").entries, 0);
}

#[test]
fn an_answer_older_than_the_time_to_live_is_not_an_answer_any_more() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path(), 1024 * 1024, Duration::from_secs(60));
    let identity = "c".repeat(64);
    store.put(&report("run-1", &identity)).expect("stored");

    let later = Timestamp::now()
        .checked_add(jiff::Span::new().hours(2))
        .expect("two hours from now");
    let collected = store.collect(later).expect("collected");
    assert_eq!(collected.expired.len(), 1);
    assert!(collected.evicted.is_empty());
    assert!(store.get(&identity).expect("a miss").is_none());
}

#[test]
fn two_runs_of_the_same_work_do_not_do_it_twice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let path = store.lease(&"c".repeat(64));

    let mut first = lock::try_claim(&path)
        .expect("a fresh claim")
        .expect("nobody has it");
    assert!(
        lock::try_claim(&path).expect("an attempt").is_none(),
        "the second run is told the first is doing it"
    );

    let cancel = Cancel::new();
    let mut said = 0u32;
    let mut count = || said = said.saturating_add(1);
    let error = lock::claim(&path, Duration::from_millis(200), &cancel, &mut count)
        .expect_err("the owner never finished");
    assert!(matches!(error, LeaseError::TimedOut { .. }), "{error}");
    assert_eq!(said, 1, "a waiting run says so once, not once per attempt");

    first.release().expect("released");
    let mut second = lock::claim(&path, Duration::from_secs(1), &cancel, &mut || ())
        .expect("the claim is free now");
    second.release().expect("released");
}

#[test]
fn a_run_that_is_interrupted_while_waiting_stops_waiting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let path = store.lease(&"c".repeat(64));
    let _held = lock::try_claim(&path)
        .expect("a fresh claim")
        .expect("nobody has it");

    let cancel = Cancel::new();
    cancel.cancel();
    let error = lock::claim(&path, Duration::from_secs(30), &cancel, &mut || ())
        .expect_err("a cancelled wait is not a wait");
    assert!(matches!(error, LeaseError::Interrupted { .. }), "{error}");
}
