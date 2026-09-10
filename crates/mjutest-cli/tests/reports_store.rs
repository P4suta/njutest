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

use mjutest_cli::app::reports::{
    DOCUMENT_NAME, HTML_NAME, JUNIT_NAME, LATEST_ANY, LATEST_FULL, SARIF_NAME, keep, pointed_at,
    retain,
};
use mjutest_cli::report::{Report, RunKind, Verdict};

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

/// A report a run may keep: one that passes its own audit.
fn keepable(run: &str, kind: RunKind) -> Report {
    let mut report = Report::new(run, kind, mjutest_cli::config::Contract::StandardV1);
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
    report.verdict = Verdict::Insufficient;
    report
}

#[test]
fn a_run_keeps_every_projection_beside_its_document_and_points_both_indexes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let run = "20260101T000000Z-aaaaaa";
    let kept = keep(root, &keepable(run, RunKind::Full)).expect("a report a run may keep");

    for name in [
        DOCUMENT_NAME,
        HTML_NAME,
        SARIF_NAME,
        JUNIT_NAME,
        mjutest_cli::report::lines::FILE_NAME,
    ] {
        assert!(
            kept.directory.join(name).exists(),
            "the surface a team already reads is the one it will read this on, and a \
             projection that has to be generated later is one nobody generates: {name}"
        );
    }
    assert_eq!(
        (
            pointed_at(root, LATEST_ANY).as_deref(),
            pointed_at(root, LATEST_FULL).as_deref()
        ),
        (Some(run), Some(run)),
        "a run over the whole project is the latest of any kind and the latest full one"
    );

    let narrowed = "20260102T000000Z-bbbbbb";
    let _kept = keep(root, &keepable(narrowed, RunKind::Changed)).expect("a report");
    assert_eq!(
        (
            pointed_at(root, LATEST_ANY).as_deref(),
            pointed_at(root, LATEST_FULL).as_deref()
        ),
        (Some(narrowed), Some(run)),
        "while a run that looked at only what changed is the latest of any kind and not \
         the latest full one: a reader asking what the whole project last established \
         would otherwise be handed an answer about a handful of files"
    );
}

#[test]
fn a_report_that_fails_its_own_audit_is_not_written_at_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let run = "20260101T000000Z-aaaaaa";
    let mut wrong = keepable(run, RunKind::Full);
    wrong.verdict = Verdict::Assured;
    wrong.findings.push(mjutest_cli::report::Finding::new(
        mjutest_cli::report::FindingKind::SurvivingMutant,
        "abcdef",
        "no test noticed this",
    ));

    let refused = keep(root, &wrong).expect_err("a report that says two things at once");
    assert!(
        !root.join("reports/runs").join(run).exists() && pointed_at(root, LATEST_ANY).is_none(),
        "an assurance is the claim that nothing was found, so one carrying a finding is \
         a report nobody may be handed — and half of one on disk with no index naming it \
         is worse than none: a later collection reads the directory as a run: {refused}"
    );
}
