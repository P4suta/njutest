// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of what runs left behind on purpose.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::PathBuf;

use jiff::Timestamp;
use mjutest_cli::kept::{FILE_NAME, Ledger, SCHEMA, forget_gone, read, record};

fn when() -> Timestamp {
    Timestamp::from_second(1_700_000_000).expect("a timestamp")
}

#[test]
fn a_tree_with_no_ledger_has_kept_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(read(dir.path()), Ledger::default());
    assert_eq!(read(dir.path()).schema, SCHEMA);
    assert!(read(dir.path()).kept.is_empty());
}

#[test]
fn what_a_run_preserved_outlives_the_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = dir.path().join("mjutest-run-abc");
    std::fs::create_dir_all(&kept).expect("mkdir");

    let written =
        record(dir.path(), "run-1", when(), std::slice::from_ref(&kept)).expect("recorded");
    assert_eq!(written, dir.path().join(FILE_NAME));
    let ledger = read(dir.path());
    assert_eq!(ledger.kept.len(), 1);
    assert_eq!(ledger.kept[0].run_id, "run-1");
    assert_eq!(ledger.kept[0].path, kept.display().to_string());
    assert!(!ledger.kept[0].at.is_empty());

    record(dir.path(), "run-2", when(), &[dir.path().join("other")]).expect("recorded");
    assert_eq!(read(dir.path()).kept.len(), 2, "a ledger accumulates");
}

#[test]
fn the_same_directory_recorded_twice_is_named_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = dir.path().join("mjutest-run-abc");
    record(dir.path(), "run-1", when(), std::slice::from_ref(&kept)).expect("recorded");
    record(dir.path(), "run-2", when(), &[kept]).expect("recorded");
    let ledger = read(dir.path());
    assert_eq!(ledger.kept.len(), 1);
    assert_eq!(
        ledger.kept[0].run_id, "run-2",
        "the run that last preserved it is the one that answers for it"
    );
}

#[test]
fn a_ledger_this_release_cannot_read_authorizes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&path, "{ not a ledger").expect("write");
    assert_eq!(
        read(dir.path()),
        Ledger::default(),
        "a document this release cannot read names no directory it may remove"
    );

    record(dir.path(), "run-1", when(), &[dir.path().join("x")]).expect("recorded");
    assert_eq!(
        read(dir.path()).kept.len(),
        1,
        "and is replaced by one it can"
    );
}

#[test]
fn a_directory_that_is_gone_stops_being_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let there = dir.path().join("mjutest-run-there");
    std::fs::create_dir_all(&there).expect("mkdir");
    let gone: PathBuf = dir.path().join("mjutest-run-gone");
    record(dir.path(), "run-1", when(), &[there.clone(), gone]).expect("recorded");

    let left = forget_gone(&read(dir.path()));
    assert_eq!(left.kept.len(), 1);
    assert_eq!(left.kept[0].path, there.display().to_string());
}
