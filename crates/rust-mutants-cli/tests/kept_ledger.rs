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

    let path = dir.path().join(rust_mutants_cli::kept::FILE_NAME);
    for wrong in [
        "not a ledger\n".to_owned(),
        serde_json::json!({
            "document_type": "somebody-else/kept",
            "schema_version": 1,
            "kept": [{"path": "/somewhere", "run_id": "20260101T000000000Z"}],
        })
        .to_string(),
        serde_json::json!({
            "document_type": "rust-mutants/kept",
            "schema_version": 2,
            "kept": [{"path": "/somewhere", "run_id": "20260101T000000000Z"}],
        })
        .to_string(),
    ] {
        std::fs::write(&path, &wrong).expect("a file that is not this ledger");
        assert!(
            Ledger::read(dir.path()).kept.is_empty(),
            "a ledger this release cannot read authorises nothing: removing a directory \
             because a file nobody could parse seemed to name it is the one thing a \
             collection may never do, and a document from another program or another \
             shape is one nobody parsed. It read {wrong}"
        );
    }
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

    let second = made(dir.path(), "second");
    let both = Ledger::record(
        dir.path(),
        "20260102T000000000Z",
        &[snapshot.clone(), second.clone()],
    )
    .expect("the ledger");
    assert_eq!(
        both.kept.len(),
        2,
        "a run that kept two directories has both named: stopping at the first would \
         leave the second on the disk with nothing to look it up by"
    );
    assert_eq!(
        both.kept.last().map(|entry| entry.path.clone()),
        Some(second),
        "in the order they were kept, oldest first, because that is the order a person \
         removes them in"
    );

    let again = Ledger::record(
        dir.path(),
        "20260103T000000000Z",
        std::slice::from_ref(&snapshot),
    )
    .expect("the ledger");
    assert_eq!(
        again.kept.len(),
        2,
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

#[test]
fn a_ledger_that_cannot_be_written_is_a_failure_and_never_a_silent_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot = made(dir.path(), "snapshot");
    let nowhere = dir.path().join("occupied");
    std::fs::write(&nowhere, "a file where a directory goes").expect("the file");

    let refused = Ledger::record(
        &nowhere,
        "20260101T000000000Z",
        std::slice::from_ref(&snapshot),
    )
    .expect_err("a ledger with nowhere to go");
    assert!(
        !refused.to_string().is_empty(),
        "a directory kept and not written down is one nobody will ever remove, so a \
         ledger that could not be written is a failure the run reports rather than one \
         it swallows: {refused}"
    );

    let cleared = Ledger::clear(&nowhere).expect_err("a ledger with nowhere to go");
    assert!(
        !cleared.to_string().is_empty(),
        "and a clearing that removed the directories and could not say it had is worse: \
         the next run reads a ledger naming directories that are gone: {cleared}"
    );
}

#[test]
fn a_path_no_document_can_hold_is_a_refusal_and_never_a_ledger_that_lost_it() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let odd = dir.path().join(std::ffi::OsStr::from_bytes(b"not\xffutf8"));
        std::fs::create_dir_all(&odd).expect("a directory whose name is not text");

        let refused = Ledger::record(
            dir.path(),
            "20260101T000000000Z",
            std::slice::from_ref(&odd),
        )
        .expect_err("a path this document cannot carry");
        assert!(
            !refused.to_string().is_empty(),
            "a filesystem takes a name that is not text and this document does not, so a \
             run that kept such a directory is told rather than handed a ledger with the \
             directory missing from it: {refused}"
        );
        assert!(
            Ledger::read(dir.path()).kept.is_empty(),
            "and nothing is written: half a ledger names some of what a run kept and \
             reads as all of it"
        );
    }
}
