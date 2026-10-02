// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Snapshots: what a path may be, and a digest that is a function of the tree and nothing else.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants_sealed::{SealedError, Snapshot, SnapshotBuilder, SnapshotFault};

/// The fault building `paths` as files is refused with.
fn refused(paths: &[&str]) -> SnapshotFault {
    let mut builder = Ok(Snapshot::builder());
    for path in paths {
        builder = builder.and_then(|built: SnapshotBuilder| built.file(path, Vec::new()));
    }
    match builder.and_then(SnapshotBuilder::build) {
        Err(SealedError::SnapshotPath { fault, .. }) => fault,
        other => panic!("{paths:?} was not refused: {other:?}"),
    }
}

#[test]
fn a_path_that_is_not_relative_names_is_refused() {
    for path in ["", "/absolute", "a//b", "./a", "a/../b", "..", "a/", "a\0b"] {
        assert_eq!(refused(&[path]), SnapshotFault::NotRelative, "{path:?}");
    }
}

#[test]
fn a_path_given_twice_or_as_a_file_and_a_directory_is_refused() {
    assert_eq!(refused(&["a", "a"]), SnapshotFault::Repeated);
    assert_eq!(refused(&["a", "a/b"]), SnapshotFault::FileAndDirectory);
    assert_eq!(refused(&["a/b", "a"]), SnapshotFault::FileAndDirectory);
    let directory_then_file = Snapshot::builder()
        .directory("a")
        .and_then(|built| built.file("a", Vec::new()));
    assert!(matches!(
        directory_then_file,
        Err(SealedError::SnapshotPath {
            fault: SnapshotFault::Repeated,
            ..
        })
    ));
}

/// A snapshot of `files`, added in the order given.
fn snapshot(files: &[(&str, &str)]) -> Snapshot {
    let mut builder = Ok(Snapshot::builder());
    for (path, contents) in files {
        builder = builder
            .and_then(|built: SnapshotBuilder| built.file(path, contents.as_bytes().to_vec()));
    }
    builder
        .and_then(SnapshotBuilder::build)
        .expect("a valid snapshot")
}

#[test]
fn the_digest_is_a_function_of_the_tree_and_not_of_the_order_it_was_given_in() {
    let one = snapshot(&[("a/x", "1"), ("b", "2")]);
    let other = snapshot(&[("b", "2"), ("a/x", "1")]);
    assert_eq!(one.digest(), other.digest());
    assert_eq!(one, other);
    for changed in [
        snapshot(&[("a/x", "1"), ("b", "3")]),
        snapshot(&[("a/y", "1"), ("b", "2")]),
        snapshot(&[("a/x", "1")]),
        snapshot(&[("a/x", "1"), ("b", "2"), ("c", "")]),
        snapshot(&[("ax", "1"), ("b", "2")]),
    ] {
        assert_ne!(one.digest(), changed.digest());
    }
    let with_empty_directory = Snapshot::builder()
        .file("a/x", b"1".to_vec())
        .and_then(|built| built.file("b", b"2".to_vec()))
        .and_then(|built| built.directory("empty"))
        .and_then(SnapshotBuilder::build)
        .expect("a valid snapshot");
    assert_ne!(one.digest(), with_empty_directory.digest());
}
