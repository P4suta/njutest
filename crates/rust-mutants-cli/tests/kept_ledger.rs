// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of directories a run kept, which outlive the run that kept them.

#![expect(
    clippy::expect_used,
    reason = "the helper that makes a directory is not itself a test"
)]

use std::path::PathBuf;

use rust_mutants_cli::kept::Ledger;

/// A directory that is there, for a run to have kept.
fn made(root: &std::path::Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("a directory a run kept");
    path
}

#[test]
fn a_tree_with_no_ledger_has_kept_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(
        Ledger::read(dir.path()).kept.is_empty(),
        "a report directory nobody has kept anything under names nothing, rather than \
         failing: the answer to what a run left behind is usually nothing"
    );

    std::fs::write(dir.path().join("kept-v1.json"), "not a ledger\n").expect("a file");
    assert!(
        Ledger::read(dir.path()).kept.is_empty(),
        "and a ledger this release cannot read authorises nothing: removing a directory \
         because a file nobody could parse seemed to name it is the one thing a \
         collection may never do"
    );
}

#[test]
fn what_a_run_kept_is_named_with_the_run_that_kept_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot = made(dir.path(), "snapshot");
    let ledger = Ledger::record(
        dir.path(),
        "20260101T000000000Z",
        std::slice::from_ref(&snapshot),
    )
    .expect("the ledger");
    assert_eq!(
        ledger.kept.len(),
        1,
        "a directory a run preserved outlives the run, so something has to say which run \
         to ask about it"
    );
    assert_eq!(
        ledger.kept.first().map(|entry| entry.run_id.as_str()),
        Some("20260101T000000000Z"),
    );
    assert_eq!(
        Ledger::read(dir.path()),
        ledger,
        "and what one run wrote is what the next one reads"
    );

    let again = Ledger::record(
        dir.path(),
        "20260102T000000000Z",
        std::slice::from_ref(&snapshot),
    )
    .expect("the ledger");
    assert_eq!(
        again.kept.len(),
        1,
        "the same directory recorded twice is named once: a person reading two lines for \
         one directory would remove it and find the second line still there"
    );
    assert_eq!(
        again.kept.first().map(|entry| entry.run_id.as_str()),
        Some("20260101T000000000Z"),
        "under the run that kept it first, because that is the run whose workings it holds"
    );
}

#[test]
fn a_directory_that_went_away_stops_being_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gone = made(dir.path(), "gone");
    let here = made(dir.path(), "here");
    let _ledger = Ledger::record(
        dir.path(),
        "20260101T000000000Z",
        std::slice::from_ref(&gone),
    )
    .expect("the ledger");
    std::fs::remove_dir_all(&gone).expect("somebody removed it");

    let ledger = Ledger::record(
        dir.path(),
        "20260102T000000000Z",
        std::slice::from_ref(&here),
    )
    .expect("the ledger");
    assert_eq!(
        ledger
            .kept
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<PathBuf>>(),
        vec![here],
        "a directory that went away without this program's help is not something to keep \
         telling a person about: they would go and look for it every time"
    );
}

#[test]
fn clearing_removes_what_the_ledger_names_and_says_how_many() {
    let dir = tempfile::tempdir().expect("tempdir");
    let one = made(dir.path(), "one");
    let two = made(dir.path(), "two");
    let _ledger = Ledger::record(
        dir.path(),
        "20260101T000000000Z",
        &[one.clone(), two.clone()],
    )
    .expect("the ledger");

    let (removed, left) = Ledger::clear(dir.path()).expect("the ledger");
    assert_eq!(
        (removed, left.kept.len(), one.exists(), two.exists()),
        (2, 0, false, false),
        "clearing takes what the ledger names, all of it, and says how many: a person \
         freeing a disk needs the number to know whether it was worth it"
    );
    assert!(
        Ledger::read(dir.path()).kept.is_empty(),
        "and what it wrote back is what the next run reads: a ledger still naming \
         directories that are gone sends the next person looking for them"
    );
}
