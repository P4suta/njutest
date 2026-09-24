// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of what runs left behind on purpose.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::PathBuf;

use jiff::Timestamp;
use njutest::kept::{Ledger, SCHEMA, forget_gone, path, read, record, release};

fn when() -> Timestamp {
    Timestamp::from_second(1_700_000_000).expect("a timestamp")
}

#[test]
fn a_tree_with_no_ledger_has_kept_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ledger = read(dir.path()).expect("an absent ledger is empty");
    assert_eq!(ledger, Ledger::default());
    assert_eq!(ledger.schema, SCHEMA);
    assert!(ledger.kept.is_empty());
}

#[test]
fn what_a_run_preserved_outlives_the_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = dir.path().join("njutest-run-abc");
    std::fs::create_dir_all(&kept).expect("mkdir");

    let written =
        record(dir.path(), "run-1", when(), std::slice::from_ref(&kept)).expect("recorded");
    assert_eq!(written, path(dir.path()));
    let ledger = read(dir.path()).expect("the ledger reads");
    assert_eq!(ledger.kept.len(), 1);
    assert_eq!(ledger.kept[0].run_id, "run-1");
    assert_eq!(ledger.kept[0].path, kept.display().to_string());
    assert!(!ledger.kept[0].at.is_empty());

    record(dir.path(), "run-2", when(), &[dir.path().join("other")]).expect("recorded");
    assert_eq!(
        read(dir.path()).expect("the ledger reads").kept.len(),
        2,
        "a ledger accumulates"
    );
}

#[test]
fn the_same_directory_recorded_twice_is_named_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let kept = dir.path().join("njutest-run-abc");
    record(dir.path(), "run-1", when(), std::slice::from_ref(&kept)).expect("recorded");
    record(dir.path(), "run-2", when(), &[kept]).expect("recorded");
    let ledger = read(dir.path()).expect("the ledger reads");
    assert_eq!(ledger.kept.len(), 1);
    assert_eq!(
        ledger.kept[0].run_id, "run-2",
        "the run that last preserved it is the one that answers for it"
    );
}

#[test]
fn a_ledger_this_release_cannot_read_authorizes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = path(dir.path());
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&path, "{ not a ledger").expect("write");
    assert!(
        read(dir.path()).is_err(),
        "a document this release cannot read is reported rather than overwritten as absence"
    );
    assert!(
        record(dir.path(), "run-1", when(), &[dir.path().join("x")]).is_err(),
        "recording cannot erase the only evidence that the existing ledger is corrupt"
    );
}

#[test]
fn a_directory_that_is_gone_stops_being_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let there = dir.path().join("njutest-run-there");
    std::fs::create_dir_all(&there).expect("mkdir");
    let gone: PathBuf = dir.path().join("njutest-run-gone");
    record(dir.path(), "run-1", when(), &[there.clone(), gone]).expect("recorded");

    let ledger = read(dir.path()).expect("the ledger reads");
    let left = forget_gone(&ledger);
    assert_eq!(left.kept.len(), 1);
    assert_eq!(left.kept[0].path, there.display().to_string());
}

#[test]
fn releasing_takes_what_the_directory_says_was_kept_and_writes_back_the_rest() {
    let root = tempfile::tempdir().expect("tempdir");
    let temp = tempfile::tempdir().expect("tempdir");
    let kept = temp.path().join("njutest-run-kept");
    let unmarked = temp.path().join("njutest-run-unmarked");
    for dir in [&kept, &unmarked] {
        std::fs::create_dir_all(dir).expect("mkdir");
    }
    rust_mutants::tempowner::claim(&kept, when())
        .expect("claims")
        .keep()
        .expect("keeps");
    record(
        root.path(),
        "run-1",
        when(),
        &[kept.clone(), unmarked.clone()],
    )
    .expect("recorded");

    let (removed, left) = release(root.path()).expect("released");

    assert_eq!(
        removed, 1,
        "the one directory that vouches for its keep goes"
    );
    assert_eq!(
        std::fs::symlink_metadata(&kept)
            .map_err(|error| error.kind())
            .err(),
        Some(std::io::ErrorKind::NotFound),
        "and it is gone"
    );
    assert!(
        std::fs::symlink_metadata(&unmarked).is_ok_and(|metadata| metadata.is_dir()),
        "a path in the ledger is not authority to delete: the directory has to say it was kept"
    );
    assert_eq!(
        left.kept
            .iter()
            .map(|entry| PathBuf::from(&entry.path))
            .collect::<Vec<_>>(),
        [unmarked],
        "what it would not take stays named"
    );
    assert_eq!(
        read(root.path()).expect("reads"),
        left,
        "and that is what it wrote back"
    );
}
