// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a reader holds while a writer is replacing what it is reading.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use njutest_cli::report::{
    Limitation, Provenance, Report, RunKind, TargetRecord, TargetStatus, Verdict,
};

/// How many entries the writer puts through the destination while the reader reads it.
const ROUNDS: u64 = 400;

/// How long the reader waits for the writer before saying the writer is the problem.
///
/// The loop reads as fast as it can on purpose — a torn read is a narrow
/// window and slowing down is how you miss it — so it finishes in under a
/// second on a machine with nothing else to do and takes as long as the
/// machine makes it take when there is. What it must not do is wait forever:
/// an unbounded wait for another thread is a sixty-second hang in somebody's
/// CI that says nothing, where a bound is a failure naming which half was
/// slow. The number is generous because it is not what this test is about
/// (ADR 0023: a claim about the apparatus wants a bound, not a guess).
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
    let record = njutest_cli::evidence::store::record(
        "m1",
        &format!("run-{round}"),
        njutest_cli::evidence::store::Outcome::Survived { targets },
    );
    njutest_cli::evidence::store::write(root, &record)
        .err()
        .map(|error| error.to_string())
}

fn take_record(root: &Path) -> Option<String> {
    match njutest_cli::evidence::store::read(root, "m1") {
        Ok(Some(_whole)) => None,
        Ok(None) => Some("the store held no record where one had been written".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

fn put_checkpoint(root: &Path, round: u64) -> Option<String> {
    let mut state = njutest_cli::checkpoint::State::new("inputs-1");
    state.attempts = u32::try_from(round).unwrap_or(u32::MAX);
    state.mutants = (0..MEMBERS)
        .map(|one| njutest_cli::checkpoint::SavedMutant {
            id: format!("mutant-{one:04}"),
            disposition: "killed".to_owned(),
            killed_by: Some(format!("demo/lib/demo tests::round_{round}")),
            duration_ms: 3,
        })
        .collect();
    njutest_cli::checkpoint::write(root, &state)
        .err()
        .map(|error| error.to_string())
}

fn take_checkpoint(root: &Path) -> Option<String> {
    match njutest_cli::checkpoint::read(root, "inputs-1") {
        Ok(Some(_whole)) => None,
        Ok(None) => Some("the store held no state where one had been written".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

fn store(root: &Path) -> njutest_cli::cache::store::Store {
    njutest_cli::cache::store::Store::new(root, 64 * 1024 * 1024, Duration::from_hours(24))
}

fn put_report(root: &Path, round: u64) -> Option<String> {
    let mut report = Report::new(
        &format!("run-{round}"),
        RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
    );
    report.provenance = Provenance {
        identity: "inputs-1".to_owned(),
        cached: false,
        source_run_id: None,
    };
    "demo".clone_into(&mut report.repository.root_name);
    report.repository.workspace_digest = "a".repeat(64);
    report.repository.configuration_digest = "b".repeat(64);
    "rustc 1.98.0".clone_into(&mut report.toolchain.rustc);
    report.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "the tree a test builds is not a git repository",
    ));
    report.verdict = Verdict::Assured;
    report.accounting.mutants.cataloged = 1;
    report.accounting.mutants.executed = 1;
    report.accounting.mutants.killed = 1;
    report.accounting.mutants.observers.tests = 1;
    report.accounting.targets.selected = u32::try_from(MEMBERS).unwrap_or(u32::MAX);
    report.accounting.targets.passed = u32::try_from(MEMBERS).unwrap_or(u32::MAX);
    report.targets = (0..MEMBERS)
        .map(|one| TargetRecord {
            id: format!("demo/lib/target-{one:04}"),
            package: "demo".to_owned(),
            name: format!("target-{one:04}"),
            status: TargetStatus::Passed,
            duration_ms: 1,
            message: None,
        })
        .collect();
    store(root)
        .put(&report)
        .err()
        .map(|error| error.to_string())
}

fn take_report(root: &Path) -> Option<String> {
    match store(root).get("inputs-1") {
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
        let done = Arc::new(AtomicBool::new(false));
        let writing = {
            let root = root.clone();
            let done = Arc::clone(&done);
            let put = kept.put;
            std::thread::spawn(move || {
                let mut refused: Vec<String> = Vec::new();
                for round in 1..=ROUNDS {
                    if let Some(stopped) = put(&root, round) {
                        refused.push(stopped);
                    }
                }
                done.store(true, Ordering::SeqCst);
                refused
            })
        };
        let mut reads: u64 = 0;
        let mut torn: Vec<String> = Vec::new();
        let giving_up = std::time::Instant::now().checked_add(PATIENCE);
        while !done.load(Ordering::SeqCst) {
            assert!(
                giving_up.is_none_or(|at| std::time::Instant::now() < at),
                "{}: the writer had not finished {ROUNDS} rounds after {PATIENCE:?}, and \
                 waiting longer would say nothing about tearing. {reads} reads so far",
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
