// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The answers earlier runs reached: what is stored, what is refused, what is read back, and what happens when two runs want the same one at once.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use njutest::cache::lock::{self, LeaseError};
use njutest::cache::store::{CacheError, Store};
use njutest::report::{Provenance, Report, RunKind, TargetRecord, TargetStatus, Verdict};
use njutest_devkit::thread::ScopedThread;
use rust_mutants::id::HexDigest;
use rust_mutants::runner::Cancel;

struct RefusingWriter;

impl Write for RefusingWriter {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed receiver"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct RefusingReader;

impl Read for RefusingReader {
    fn read(&mut self, _bytes: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed sender"))
    }
}

fn digest(value: &str) -> HexDigest {
    HexDigest::try_from(value).expect("a canonical digest")
}

fn report(run_id: &str, identity: &str) -> Report {
    let mut source = njutest::report::BuildReport::new(
        "cache-evidence",
        RunKind::Full,
        njutest::config::Contract::StandardV1,
    );
    source.provenance = Provenance {
        identity: identity.to_owned(),
        facts: njutest::report::Established::Here,
    };
    "demo".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.started);
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.finished);
    source.limitations.push(njutest::report::Limitation::new(
        "git-metadata-unavailable",
        "the tree a test builds is not a git repository",
    ));
    source.targets.push(TargetRecord {
        id: "demo/lib/demo".to_owned(),
        package: "demo".to_owned(),
        name: "demo".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    source.count_targets().expect("one exact target row");
    source.mutants.push(njutest::report::MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "c".repeat(20),
        path: "src/lib.rs".to_owned(),
        position: njutest::report::Position {
            line: 1,
            column: 1,
            character_column: 1,
        },
        rule: "gt-to-ge".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: String::new(),
        outcome: njutest::report::Decided::Killed {
            by: "demo/lib/demo".to_owned(),
        },
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    });
    source.accounting.mutants = njutest::report::MutantAccounting {
        cataloged: 1,
        executed: 1,
        killed: 1,
        observers: njutest::report::ObserverAccounting {
            tests: 1,
            ..njutest::report::ObserverAccounting::default()
        },
        ..njutest::report::MutantAccounting::default()
    };
    njutest::testkit::read_every_named_file(&mut source);
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let final_run = rust_mutants::id::RunId::try_from(run_id).expect("a canonical run id");
    let latticed = njutest::report::across::configured(&final_run, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog cache fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
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
            .get(&digest(&identity))
            .expect("a miss is not a failure")
            .is_none(),
        "nothing was stored yet"
    );

    store.put(&report("run-1", &identity)).expect("stored");
    assert!(store.entry(&digest(&identity)).is_file());
    let read = store
        .get(&digest(&identity))
        .expect("stored")
        .expect("what was stored is there");
    assert_eq!(read.run_id(), "run-1");
    assert_eq!(read.verdict(), Verdict::Assured);
    assert!(
        store
            .get(&digest(&"d".repeat(64)))
            .expect("a miss")
            .is_none(),
        "a different identity is a different question"
    );
}

#[test]
fn an_answer_that_is_not_the_answer_it_claims_to_be_is_never_quietly_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let identity = "c".repeat(64);
    store.put(&report("run-1", &identity)).expect("stored");

    std::fs::write(store.entry(&digest(&identity)), "{ not a report").expect("write");
    let error = store
        .get(&digest(&identity))
        .expect_err("a document that does not parse");
    assert!(matches!(error, CacheError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("NJ8004"), "{error}");

    let misfiled = report("run-1", &"e".repeat(64));
    let text = njutest::report::json::render(&misfiled).expect("render");
    std::fs::write(store.entry(&digest(&identity)), text).expect("write");
    let error = store
        .get(&digest(&identity))
        .expect_err("an answer filed under the wrong question");
    assert!(error.to_string().contains(&identity), "{error}");

    let sound = njutest::report::json::render(&report("run-1", &identity)).expect("render");
    let unsound = sound.replace("\"selected\": 1", "\"selected\": 9");
    std::fs::write(store.entry(&digest(&identity)), unsound).expect("write");
    let error = store
        .get(&digest(&identity))
        .expect_err("an answer no reader could check");
    assert!(matches!(error, CacheError::Corrupt { .. }), "{error}");
}

#[test]
fn a_report_that_answers_for_no_inputs_or_was_itself_read_back_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());

    let nameless = report("run-1", njutest::report::UNAVAILABLE);
    let error = store.put(&nameless).expect_err("no identity");
    assert!(matches!(error, CacheError::Refused { .. }), "{error}");

    let copied = report("run-2", &"c".repeat(64))
        .read_back_as(&rust_mutants::id::RunId::try_from("run-1").expect("a canonical run id"))
        .expect("a report read back under a new run");
    let error = store.put(&copied).expect_err("already stored elsewhere");
    assert!(
        error.to_string().contains("read back"),
        "a chain of copies is not a chain of evidence: {error}"
    );

    let unfiled = report("run-3", "not a digest any store could file under");
    let error = store
        .put(&unfiled)
        .expect_err("an answer keyed by no question is refused");
    assert!(matches!(error, CacheError::Refused { .. }), "{error}");
    assert!(
        store
            .get(&digest(&"c".repeat(64)))
            .expect("a miss")
            .is_none(),
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
    assert!(store.get(&digest(&identity)).expect("a miss").is_none());
}

#[test]
fn expiration_and_size_bounds_include_their_exact_edges() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "c".repeat(64);
    let ttl = Store::new(dir.path(), u64::MAX, Duration::from_secs(60));
    ttl.put(&report("run-1", &identity)).expect("stored");
    let path = ttl.entry(&digest(&identity));
    let modified = Timestamp::try_from(
        std::fs::metadata(&path)
            .expect("entry metadata")
            .modified()
            .expect("entry modification time"),
    )
    .expect("a representable modification time");
    let just_before = modified
        .checked_add(jiff::Span::new().seconds(59))
        .expect("59 seconds later");
    assert!(
        ttl.collect(just_before)
            .expect("collected before the edge")
            .expired
            .is_empty(),
        "an answer is live until its entire TTL has elapsed"
    );
    let edge = modified
        .checked_add(jiff::Span::new().seconds(60))
        .expect("60 seconds later");
    let expired = ttl.collect(edge).expect("collected at the edge");
    assert_eq!(expired.expired, [path]);
    assert!(expired.bytes > 0, "removed bytes are accounted for");

    let sizes = tempfile::tempdir().expect("tempdir");
    let unbounded = store(sizes.path());
    for (run, byte) in [("run-1", 'd'), ("run-2", 'e')] {
        let identity = byte.to_string().repeat(64);
        unbounded.put(&report(run, &identity)).expect("stored");
    }
    let before = unbounded.status().expect("two entries");
    assert_eq!(before.entries, 2);
    let exact = Store::new(sizes.path(), before.bytes, Duration::ZERO);
    assert!(
        exact
            .collect(Timestamp::now())
            .expect("exactly bounded")
            .evicted
            .is_empty(),
        "a store whose entries equal its byte bound is within the bound"
    );
    let one_byte_short = Store::new(sizes.path(), before.bytes.saturating_sub(1), Duration::ZERO);
    let evicted = one_byte_short
        .collect(Timestamp::now())
        .expect("one byte over the bound");
    assert_eq!(evicted.evicted.len(), 1);
    assert!(evicted.bytes > 0);
    assert_eq!(unbounded.status().expect("one entry remains").entries, 1);
}

#[test]
fn zero_ttl_and_zero_size_bound_both_mean_unbounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = Store::new(dir.path(), 0, Duration::ZERO);
    let identity = "c".repeat(64);
    kept.put(&report("run-1", &identity)).expect("stored");
    let far_future = Timestamp::now()
        .checked_add(jiff::Span::new().hours(24 * 365 * 100))
        .expect("a century later");
    let collected = kept.collect(far_future).expect("unbounded collection");
    assert_eq!(collected, njutest::cache::store::Collected::default());
    assert_eq!(kept.status().expect("still stored").entries, 1);
}

#[test]
fn two_runs_of_the_same_work_do_not_do_it_twice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let path = store.lease(&digest(&"c".repeat(64)));

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
fn a_lease_releases_on_drop_and_explicit_release_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = store(dir.path()).lease(&digest(&"c".repeat(64)));
    {
        let lease = lock::try_claim(&path)
            .expect("a usable lock")
            .expect("a fresh claim");
        assert!(lock::try_claim(&path).expect("contended").is_none());
        drop(lease);
    }
    let mut reclaimed = lock::try_claim(&path)
        .expect("a usable lock")
        .expect("Drop released the claim");
    reclaimed.release().expect("released once");
    reclaimed.release().expect("released twice");
}

#[test]
fn an_unusable_lease_parent_is_an_error_and_never_a_panic_or_a_claim() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().join("not-a-directory");
    std::fs::write(&parent, "file\n").expect("a file in place of a directory");
    let path = parent.join("entry.lock");
    let error = lock::try_claim(&path).expect_err("the parent cannot be created");
    assert!(
        matches!(&error, LeaseError::Unusable { path: named, .. } if named == &path),
        "{error}"
    );
}

#[test]
fn timeout_equality_is_expired_and_a_contended_claim_polls() {
    assert!(!lock::timed_out(
        Duration::from_nanos(9),
        Duration::from_nanos(10)
    ));
    assert!(lock::timed_out(
        Duration::from_nanos(10),
        Duration::from_nanos(10)
    ));
    assert!(lock::timed_out(
        Duration::from_nanos(11),
        Duration::from_nanos(10)
    ));

    let dir = tempfile::tempdir().expect("tempdir");
    let path = store(dir.path()).lease(&digest(&"d".repeat(64)));
    let held = lock::try_claim(&path)
        .expect("usable")
        .expect("fresh claim");
    let cancel = Cancel::new();
    let (started_waiting, release_owner) = std::sync::mpsc::sync_channel(0);
    std::thread::scope(|scope| {
        let release = ScopedThread::launch(scope, move || {
            release_owner
                .recv()
                .expect("the contender reached the wait");
            std::thread::sleep(Duration::from_millis(10));
            drop(held);
        });
        let started = Instant::now();
        let mut announced = false;
        let mut waiting = || {
            if !announced {
                announced = true;
                started_waiting.send(()).expect("tell the owner");
            }
        };
        let lease = lock::claim(&path, Duration::from_secs(2), &cancel, &mut waiting)
            .expect("reclaimed after the owner left");
        assert!(announced, "contention is announced exactly once");
        assert!(
            started.elapsed() >= lock::POLL,
            "a contender waits for the polling interval instead of spinning"
        );
        release.join().expect("the lease owner leaves");
        drop(lease);
    });
}

#[test]
fn a_run_that_is_interrupted_while_waiting_stops_waiting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let path = store.lease(&digest(&"c".repeat(64)));
    let held = lock::try_claim(&path)
        .expect("a fresh claim")
        .expect("nobody has it");

    let cancel = Cancel::new();
    cancel.cancel();
    let error = lock::claim(&path, Duration::from_secs(30), &cancel, &mut || ())
        .expect_err("a cancelled wait is not a wait");
    assert!(matches!(error, LeaseError::Interrupted { .. }), "{error}");
    drop(held);
}

#[test]
fn an_entry_that_is_not_there_is_no_answer_and_an_entry_that_cannot_be_read_is_a_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let identity = "a".repeat(64);

    assert!(
        matches!(store.get(&digest(&identity)), Ok(None)),
        "no earlier run answered this question here, which is what an empty store is"
    );

    let path = store.entry(&digest(&identity));
    std::fs::create_dir_all(&path).expect("a directory where an entry goes");
    let error = store
        .get(&digest(&identity))
        .expect_err("an entry that is not a file");
    assert!(
        matches!(&error, CacheError::Unusable { path: named, .. } if named == &path),
        "and an entry this could not read for any other reason is not the same as one \
         that is not there: reading it as absent would establish everything again and \
         call that a fresh answer, with nothing anywhere saying the store had stopped \
         working: {error}"
    );
}

#[test]
fn listing_failures_and_non_file_entries_fail_closed_for_every_store_operation() {
    let blocked = tempfile::tempdir().expect("tempdir");
    let unusable = store(blocked.path());
    std::fs::create_dir_all(unusable.root().parent().expect("layout parent"))
        .expect("layout parent");
    std::fs::write(unusable.root(), "not a directory\n").expect("blocked store root");
    assert!(matches!(
        unusable.status(),
        Err(CacheError::Unusable { .. })
    ));
    assert!(matches!(
        unusable.collect(Timestamp::now()),
        Err(CacheError::Unusable { .. })
    ));
    assert!(matches!(
        unusable.export(&mut Vec::new()),
        Err(CacheError::Unusable { .. })
    ));

    let malformed = tempfile::tempdir().expect("tempdir");
    let corrupt = store(malformed.path());
    std::fs::create_dir_all(corrupt.entry(&digest(&"a".repeat(64))))
        .expect("a directory named like an entry");
    assert!(matches!(corrupt.status(), Err(CacheError::Corrupt { .. })));
    assert!(matches!(
        corrupt.collect(Timestamp::now()),
        Err(CacheError::Corrupt { .. })
    ));
    assert!(matches!(
        corrupt.export(&mut Vec::new()),
        Err(CacheError::Corrupt { .. })
    ));
}

fn keepable() -> Report {
    report("20260905t081500z-abcdef", &"c".repeat(64))
}

#[test]
fn a_report_the_store_may_not_keep_says_which_of_the_three_reasons_it_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = store(dir.path());

    let nameless = report("20260905t081500z-abcdef", njutest::report::UNAVAILABLE);
    let copied = keepable()
        .read_back_as(
            &rust_mutants::id::RunId::try_from("20260905t081500z-000000")
                .expect("a canonical run id"),
        )
        .expect("a report read back under a new run");

    for (what, report, says) in [
        (
            "a report with no identity",
            &nameless,
            "answers for no inputs",
        ),
        (
            "a report that was itself read back",
            &copied,
            "already stored where it came from",
        ),
    ] {
        let refused = kept.put(report).expect_err(what);
        assert!(
            matches!(&refused, CacheError::Refused { message } if message.contains(says)),
            "{what} is one the store may not keep, and which of the three it is decides \
             what somebody does about it. A report with more than one thing wrong is \
             quoted from the first of them, because that is the one to fix: {refused}"
        );
    }

    assert!(
        kept.put(&keepable()).is_ok(),
        "and a report that is none of those is what the store is for"
    );
}

#[test]
fn a_report_that_cannot_be_written_names_the_path_that_refused_it() {
    let report = keepable();
    let identity = report.provenance().identity.clone();

    let other = tempfile::tempdir().expect("tempdir");
    let elsewhere = store(&other.path().join("root"));
    std::fs::create_dir_all(elsewhere.entry(&digest(&identity)))
        .expect("a directory where the entry goes");
    let refused = elsewhere
        .put(&report)
        .expect_err("an entry that is a directory");
    assert!(
        matches!(&refused, CacheError::Unusable { path, .. } if path == &elsewhere.entry(&digest(&identity))),
        "an answer is written beside its own name and moved into place, so one that was \
         written and could not be moved names where it was going: {refused}"
    );
}

#[test]
fn what_one_machine_established_is_carried_to_another_and_answers_there() {
    let here = tempfile::tempdir().expect("tempdir");
    let there = tempfile::tempdir().expect("tempdir");
    let (one, two) = ("d".repeat(64), "e".repeat(64));
    store(here.path())
        .put(&report("20260909t000000z-aaaaaa", &one))
        .expect("the first answer");
    store(here.path())
        .put(&report("20260909t000001z-bbbbbb", &two))
        .expect("the second answer");

    let mut carried = Vec::new();
    let written = store(here.path())
        .export(&mut carried)
        .expect("what this machine knows");
    assert_eq!(
        written, 2,
        "a matrix of jobs that each rebuild what one of them already answered pays for \
         the same work as many times as it has jobs, so what leaves a machine is every \
         answer it holds"
    );
    let identities: Vec<String> = String::from_utf8(carried.clone())
        .expect("the carried stream is text")
        .lines()
        .map(|line| {
            njutest::report::json::parse(line)
                .expect("one report per line")
                .provenance()
                .identity
                .clone()
        })
        .collect();
    assert_eq!(
        identities,
        [one.clone(), two.clone()],
        "exports are stable in identity order"
    );

    let read = store(there.path())
        .import(&mut carried.as_slice())
        .expect("what the other machine now knows");
    assert_eq!(read, 2, "and every one of them arrives");
    for identity in [&one, &two] {
        assert!(
            store(there.path())
                .get(&digest(identity))
                .expect("the store answers")
                .is_some_and(|report| &report.provenance().identity == identity),
            "and answers there for the inputs it answered for here, which is the whole \
             of what carrying it is for"
        );
    }
}

#[test]
fn stopped_import_and_export_streams_are_reported_as_transport_failures() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = store(dir.path());
    kept.put(&keepable()).expect("one answer to export");
    assert!(matches!(
        kept.export(&mut RefusingWriter),
        Err(CacheError::Carrying { .. })
    ));
    assert!(matches!(
        kept.import(&mut RefusingReader),
        Err(CacheError::Carrying { .. })
    ));
}

#[test]
fn blank_import_lines_are_skipped_without_hiding_later_answers_or_line_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = store(dir.path());
    let one = njutest::report::json::line(&report("run-1", &"1".repeat(64))).expect("first line");
    let two = njutest::report::json::line(&report("run-2", &"2".repeat(64))).expect("second line");
    let stream = format!("\n{one}\n\n{two}\n");
    assert_eq!(kept.import(&mut stream.as_bytes()).expect("two answers"), 2);

    let broken = format!("\n{one}\nnot-json\n");
    let error = kept
        .import(&mut broken.as_bytes())
        .expect_err("the third physical line is malformed");
    assert!(
        matches!(error, CacheError::Arriving { line: 3, .. }),
        "{error}"
    );
}

#[test]
fn an_answer_a_machine_cannot_vouch_for_is_not_carried_to_another_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let identity = "f".repeat(64);
    store
        .put(&report("20260909t000000z-aaaaaa", &identity))
        .expect("an answer");
    std::fs::write(store.entry(&digest(&identity)), "{ not a report }")
        .expect("the entry, spoiled");

    let mut carried = Vec::new();
    let refused = store.export(&mut carried).expect_err("a refusal");
    assert!(
        matches!(refused, CacheError::Corrupt { .. }),
        "an entry a machine cannot read back is not one it may hand to another machine: \
         copying it makes one broken answer into two, and the second machine has no way \
         left to know where it came from: {refused}"
    );
    assert!(
        refused.to_string().contains(&identity),
        "and it names the entry, which is the one thing a person can remove: {refused}"
    );
    assert!(
        carried.is_empty() || !carried.is_empty(),
        "the stream is whatever was written before the refusal; the refusal is the claim"
    );
}

#[test]
fn what_arrives_from_another_machine_is_held_to_what_a_run_of_this_one_would_be() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(dir.path());
    let carried = report("20260909t000000z-aaaaaa", &"1".repeat(64))
        .read_back_as(
            &rust_mutants::id::RunId::try_from("20260909t000001z-bbbbbb")
                .expect("a canonical run id"),
        )
        .expect("a report read back under a new run");
    let line = njutest::report::json::line(&carried).expect("a report as one line");

    let refused = store.import(&mut line.as_bytes()).expect_err("a refusal");
    assert!(
        matches!(refused, CacheError::Refused { .. }),
        "what a machine may store is what a run may store, and an answer that arrived \
         over the network gets no weaker a check than one this machine established: \
         {refused}"
    );

    let refused = store
        .import(&mut b"not a report at all\n".as_slice())
        .expect_err("a refusal");
    assert!(
        matches!(refused, CacheError::Arriving { line: 1, .. }),
        "and a stream that is not answers at all is refused rather than skipped: \
         {refused}"
    );
    assert!(
        refused.to_string().contains('1'),
        "naming the line, because that is where a person looks: {refused}"
    );
    assert_eq!(
        store.status().expect("the store").entries,
        0,
        "and nothing that failed the check is left behind"
    );

    let spread = njutest::report::json::render(&report("20260909t000000z-aaaaaa", &"2".repeat(64)))
        .expect("a report a person can read");
    let refused = store.import(&mut spread.as_bytes()).expect_err("a refusal");
    assert!(
        matches!(refused, CacheError::Arriving { line: 1, .. }),
        "one line is one answer, so a document laid out for a person to read is refused \
         at its first line rather than taken for as many answers as it has lines: \
         {refused}"
    );
}
