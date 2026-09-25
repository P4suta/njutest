// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a reader holds while a writer is replacing what it is reading.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use njutest::report::{BuildReport, Limitation, Provenance, RunKind, TargetRecord, TargetStatus};
use njutest_devkit::thread::JoinedThread;
use rust_mutants::id::HexDigest;

fn digest(number: u8) -> HexDigest {
    HexDigest::try_from(format!("{number:064x}")).expect("a canonical digest")
}

/// How many entries the writer puts through the destination while the reader reads it.
const ROUNDS: u64 = 400;

/// How long the reader waits for the writer to finish one more round before saying the writer is the problem.
///
/// The loop reads as fast as it can on purpose — a torn read is a narrow window and slowing down is how you miss it — so it finishes in under a second on a machine with nothing else to do and takes as long as the machine makes it take when there is.
/// What it must not do is wait forever:
/// an unbounded wait for another thread is a sixty-second hang in somebody's CI that says nothing, where a bound is a failure naming which half was slow.
/// The bound is on a round, not on all of them (ADR 0026): a writer that keeps finishing rounds on a busy machine is slow, not stuck, and was failed at 30 s for all 400 while the merge queue's gate ran at load 60.
const PATIENCE: Duration = Duration::from_secs(30);

/// How many members one entry carries, so that writing one is not a single small write.
const MEMBERS: u64 = 300;

/// One store a later run reads back, as the writer that fills it and the reader that takes what is there.
struct Kept {
    /// What the store is called where a person meets it.
    name: &'static str,
    /// Puts entry number `round` where the reader looks, and says what stopped it.
    put: fn(&Path, u64) -> Option<String>,
    /// What stopped the reader, or nothing when it held a whole entry.
    take: fn(&Path) -> Option<String>,
}

/// The three of them.
const KEPT: &[Kept] = &[
    Kept {
        name: "what earlier runs established about a mutant",
        put: put_record,
        take: take_record,
    },
    Kept {
        name: "the checkpoint an interrupted run left",
        put: put_checkpoint,
        take: take_checkpoint,
    },
    Kept {
        name: "the reports a cache keeps",
        put: put_report,
        take: take_report,
    },
];

fn put_record(root: &Path, round: u64) -> Option<String> {
    let targets: BTreeMap<String, String> = (0..MEMBERS)
        .map(|one| (format!("demo/lib/target-{one:04}"), format!("{round:064}")))
        .collect();
    let record = njutest::evidence::store::record(
        digest(1),
        &format!("run-{round}"),
        njutest::evidence::store::Outcome::Survived { targets },
    );
    match njutest::evidence::store::write(root, &record) {
        Ok(_written) => None,
        Err(error) => Some(error.to_string()),
    }
}

fn take_record(root: &Path) -> Option<String> {
    match njutest::evidence::store::read(root, &digest(1)) {
        Ok(Some(_whole)) => None,
        Ok(None) => Some("the store held no record where one had been written".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

fn put_checkpoint(root: &Path, round: u64) -> Option<String> {
    let identity = "a".repeat(64);
    let mut state = njutest::checkpoint::State::new(&identity);
    state.attempts = u32::try_from(round).unwrap_or(u32::MAX).max(1);
    state.mutants = (0..MEMBERS)
        .map(|one| njutest::checkpoint::SavedMutant {
            id: format!("{one:064x}"),
            disposition: njutest::checkpoint::SavedDisposition::Killed {
                by: format!("demo/lib/demo tests::round_{round}"),
                before: Vec::new(),
            },
            duration_ms: 3,
        })
        .collect();
    match njutest::checkpoint::write(root, &state) {
        Ok(_written) => None,
        Err(error) => Some(error.to_string()),
    }
}

fn take_checkpoint(root: &Path) -> Option<String> {
    let identity = "a".repeat(64);
    match njutest::checkpoint::read(root, &identity) {
        Ok(Some(_whole)) => None,
        Ok(None) => Some("the store held no state where one had been written".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

fn store(root: &Path) -> njutest::cache::store::Store {
    njutest::cache::store::Store::new(root, 64 * 1024 * 1024, Duration::from_hours(24))
}

fn put_report(root: &Path, round: u64) -> Option<String> {
    let mut source = BuildReport::new(
        &format!("run-{round}"),
        RunKind::Full,
        njutest::config::Contract::StandardV1,
    );
    source.provenance = Provenance {
        identity: digest(1).to_string(),
        facts: njutest::report::Established::Here,
    };
    "demo".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.started);
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.finished);
    source.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "the tree a test builds is not a git repository",
    ));
    source.targets = (0..MEMBERS)
        .map(|one| TargetRecord {
            id: format!("demo/lib/target-{one:04}"),
            package: "demo".to_owned(),
            name: format!("target-{one:04}"),
            status: TargetStatus::Passed,
            duration_ms: 1,
            message: None,
        })
        .collect();
    source
        .count_targets()
        .expect("one exact target row per member");
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let final_run = rust_mutants::id::RunId::try_from(format!("final-{round}").as_str())
        .expect("a canonical run id");
    let latticed = njutest::report::across::configured(&final_run, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog cache fixture cannot be a shard");
    };
    let report = latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion");
    match store(root).put(&report) {
        Ok(()) => None,
        Err(error) => Some(error.to_string()),
    }
}

fn take_report(root: &Path) -> Option<String> {
    match store(root).get(&digest(1)) {
        Ok(Some(_whole)) => None,
        Ok(None) => Some("the store held no report where one had been written".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

#[test]
fn an_entry_a_reader_takes_while_a_run_replaces_it_is_one_whole_entry() {
    for kept in KEPT {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        assert!(
            (kept.put)(&root, 0).is_none(),
            "{}: the entry a reader starts from",
            kept.name
        );
        let finished = Arc::new(AtomicU64::new(0));
        let writing = {
            let root = root.clone();
            let finished = Arc::clone(&finished);
            let put = kept.put;
            JoinedThread::launch(move || {
                let mut refused: Vec<String> = Vec::new();
                for round in 1..=ROUNDS {
                    if let Some(stopped) = put(&root, round) {
                        refused.push(stopped);
                    }
                    finished.store(round, Ordering::SeqCst);
                }
                refused
            })
        };
        let mut reads: u64 = 0;
        let mut torn: Vec<String> = Vec::new();
        let mut seen: u64 = 0;
        let mut giving_up = std::time::Instant::now().checked_add(PATIENCE);
        loop {
            let rounds = finished.load(Ordering::SeqCst);
            if rounds == ROUNDS {
                break;
            }
            if rounds > seen {
                seen = rounds;
                giving_up = std::time::Instant::now().checked_add(PATIENCE);
            }
            assert!(
                giving_up.is_none_or(|at| std::time::Instant::now() < at),
                "{}: the writer finished no round in {PATIENCE:?} after its {seen}th of \
                 {ROUNDS}, and waiting longer would say nothing about tearing. {reads} reads \
                 so far",
                kept.name
            );
            reads = reads.saturating_add(1);
            if let Some(stopped) = (kept.take)(&root) {
                torn.push(stopped);
            }
        }
        let refused = writing.join().expect("the writer");
        assert!(
            refused.is_empty(),
            "{}: every entry the writer put was one the store took: {refused:?}",
            kept.name
        );
        assert!(
            torn.is_empty(),
            "{}: a reader that arrives while a run is replacing an entry holds the entry \
             that was there or the one replacing it, and never the seam between them. A \
             half-written entry is either refused, which throws away an answer nobody \
             contradicted, or read as a prefix that happens to parse, which is one store \
             saying two things. {reads} reads, {} of them torn, the first: {}",
            kept.name,
            torn.len(),
            torn.first().map_or("", String::as_str)
        );
    }
}
