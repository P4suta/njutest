// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run keeps of itself, and what a later run may take away.
//!
//! Every one of these is reached here directly. Driving `verify` instead means
//! starting a run, and the rule a collection rests on — that the newest is
//! kept and the ones the indexes name are never taken — is one no run exercises
//! until a directory has more in it than anybody wants to make in a test.

#![expect(
    clippy::expect_used,
    reason = "the helper that fills a directory with run directories is not itself a test"
)]

use std::path::{Path, PathBuf};

use mjutest_cli::app::reports::{LATEST_ANY, LATEST_FULL, pointed_at, retain};

/// A workspace with one directory per run named, and an index pointing where asked.
fn filled(runs: &[&str], indexes: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    for run in runs {
        std::fs::create_dir_all(root.join("reports/runs").join(run)).expect("a run directory");
    }
    for (index, run) in indexes {
        let path = root.join(index);
        std::fs::create_dir_all(path.parent().unwrap_or(&root)).expect("the index's directory");
        std::fs::write(
            &path,
            serde_json::json!({
                "schema": "mjutest-report-index-v1",
                "run_id": run,
                "directory": format!("reports/runs/{run}"),
            })
            .to_string(),
        )
        .expect("the index");
    }
    (dir, root)
}

/// The names of the run directories still there.
fn left(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("reports/runs"))
        .expect("the runs directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_collection_keeps_the_newest_and_says_what_it_took() {
    let runs = [
        "20260101T000000Z-aaaaaa",
        "20260102T000000Z-bbbbbb",
        "20260103T000000Z-cccccc",
        "20260104T000000Z-dddddd",
    ];
    let (_dir, root) = filled(&runs, &[]);

    let removed = retain(&root, 2);
    assert_eq!(
        left(&root),
        vec![runs[2].to_owned(), runs[3].to_owned()],
        "a run identity starts with the time it began, so the newest two are the last \
         two in order: keeping the first two would keep the ones nobody is looking at"
    );
    assert_eq!(
        removed.len(),
        2,
        "and the collection says what it took, because a person watching a directory \
         shrink wants to know it was this and not something else: {removed:?}"
    );
    assert!(
        removed.iter().all(|path| path
            .file_name()
            .is_some_and(|name| name == runs[0] || name == runs[1])),
        "naming each one: {removed:?}"
    );
}

#[test]
fn a_collection_never_takes_a_run_an_index_still_names() {
    let runs = [
        "20260101T000000Z-aaaaaa",
        "20260102T000000Z-bbbbbb",
        "20260103T000000Z-cccccc",
    ];
    let (_dir, root) = filled(&runs, &[(LATEST_ANY, runs[2]), (LATEST_FULL, runs[0])]);

    let _removed = retain(&root, 1);
    let kept = left(&root);
    assert!(
        kept.contains(&runs[0].to_owned()),
        "the oldest is what an index names, and an index pointing at a directory a \
         collection took is a reader sent to nothing: `mjutest report` would answer \
         about a run whose report is gone. It kept {kept:?}"
    );
    assert!(
        kept.contains(&runs[2].to_owned()),
        "the newest is kept because it is the newest, and also because the other index \
         names it: {kept:?}"
    );
    assert!(
        !kept.contains(&runs[1].to_owned()),
        "and the one in the middle, which nothing points at and nothing is the newest \
         of, is the one a collection is for: {kept:?}"
    );
}

#[test]
fn a_collection_asked_to_keep_everything_takes_nothing() {
    let runs = ["20260101T000000Z-aaaaaa", "20260102T000000Z-bbbbbb"];
    let (_dir, root) = filled(&runs, &[]);
    assert!(
        retain(&root, u32::MAX).is_empty() && left(&root).len() == 2,
        "a store nobody bounded is one a person is keeping on purpose: taking anything \
         from it would be this program deciding how much history somebody may have"
    );
    assert!(
        retain(&root, 0).len() == 2 && left(&root).is_empty(),
        "while one bounded at nothing keeps nothing, and says so rather than quietly \
         treating zero as one"
    );
}

#[test]
fn an_index_that_names_nothing_is_read_as_naming_nothing() {
    let (_dir, root) = filled(&["20260101T000000Z-aaaaaa"], &[]);
    assert_eq!(
        pointed_at(&root, LATEST_ANY),
        None,
        "a directory where no run has finished has no latest run, and answering with one \
         would send a reader to a report nobody wrote"
    );

    std::fs::write(root.join(LATEST_ANY), "not an index\n").expect("a file that is not one");
    assert_eq!(
        pointed_at(&root, LATEST_ANY),
        None,
        "and an index this release cannot read is one that names nothing rather than one \
         that names whatever the bytes happen to look like"
    );

    let (_other, named) = filled(
        &["20260101T000000Z-aaaaaa"],
        &[(LATEST_ANY, "20260101T000000Z-aaaaaa")],
    );
    assert_eq!(
        pointed_at(&named, LATEST_ANY).as_deref(),
        Some("20260101T000000Z-aaaaaa"),
        "while one that names a run answers with it"
    );
}
