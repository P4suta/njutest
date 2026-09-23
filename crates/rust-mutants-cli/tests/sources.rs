// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading back the files a run measured, and telling them from the ones somebody has edited since.

#![expect(
    clippy::expect_used,
    reason = "the helper that builds one document is not itself a test, and a tree that \
              cannot be written is a setup failure to report by panicking"
)]

use rust_mutants::outcome::Outcome;
use rust_mutants::report::catalog::{PlatformDocument, SelectionDocument, WorkspaceDocument};
use rust_mutants_cli::report::run::{
    Accounting, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::report::sources::{Held, read};

const SOURCE: &str = "pub fn wide(n: i32) -> bool {\n    n > 1\n}\n";

fn mutant(path: &str, digest: &str) -> RunMutantDocument {
    RunMutantDocument {
        index: 0,
        id: "a".repeat(64),
        display_id: "a".repeat(20),
        path: path.to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
        item: "demo".to_owned(),
        rule_version: 1,
        line: 2,
        column: 7,
        start_byte: 35,
        end_byte: 36,
        source_digest: digest.to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: Outcome::Killed,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 1,
        tests_run: Some(1),
        killed_by: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        identical: rust_mutants::run::CodegenIdentity::NotMeasured,
        retried: false,
        expected: false,
        unreached: false,
        source_run_id: None,
        step_notice: None,
    }
}

fn document(mutants: Vec<RunMutantDocument>) -> RunDocument {
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 2,
        tool_version: "0.1.0".to_owned(),
        run: RunMeta {
            id: "20260905T120000000Z".to_owned(),
            started_at: "2026-09-05T12:00:00Z".to_owned(),
            finished_at: "2026-09-05T12:00:01Z".to_owned(),
            duration_ms: 1,
            interrupted: false,
            exit_code: 0,
            shard: None,
        },
        workspace: WorkspaceDocument {
            root_name: "demo".to_owned(),
            toolchain: "rustc 1.98.0".to_owned(),
            workspace_digest: "a".repeat(64),
            catalog_digest: "b".repeat(64),
            platform: PlatformDocument {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
            },
        },
        selection: SelectionDocument {
            build: Vec::new(),
            tier: "balanced".to_owned(),
            operators: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            mutant_steps: None,
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: Accounting {
            cataloged: 1,
            refused: 0_u32.into(),
            skipped: 0_u32.into(),
            executed: 1_u32.into(),
            killed: 1_u32.into(),
            survived: 0_u32.into(),
            step_limit_reached: 0_u32.into(),
            waited: 0_u32.into(),
            inconclusive: 0_u32.into(),
            errored: 0_u32.into(),
            unreached: 0_u32.into(),
            discharged: 0_u32.into(),
            not_run: 0_u32.into(),
            expected: 0_u32.into(),
        },
        score: Some(ScoreDocument {
            detected: 1,
            decided: 1,
            value: 1.0,
        }),
        mutants,
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: Vec::new(),
        findings: Vec::new(),
    }
}

/// A tree holding `src/lib.rs` with these bytes.
fn tree(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src")).expect("a source directory");
    std::fs::write(dir.path().join("src/lib.rs"), text).expect("the library");
    dir
}

#[test]
fn a_file_whose_bytes_are_the_ones_the_run_measured_comes_back_as_measured() {
    let dir = tree(SOURCE);
    let digest = rust_mutants::id::digest(SOURCE.as_bytes());
    let held = read(&document(vec![mutant("src/lib.rs", &digest)]), dir.path())
        .expect("the file the run measured");
    assert_eq!(
        held.get("src/lib.rs"),
        Some(&Held::Measured(SOURCE.to_owned())),
        "a projection draws every mutation onto the source, so it has to know the source \
         is the one the mutation was measured in"
    );
}

#[test]
fn a_file_somebody_edited_since_the_run_is_not_the_file_the_run_measured() {
    let dir = tree("pub fn wide(n: i32) -> bool {\n    n >= 1\n}\n");
    let digest = rust_mutants::id::digest(SOURCE.as_bytes());
    let held = read(&document(vec![mutant("src/lib.rs", &digest)]), dir.path())
        .expect("the file, whatever it now holds");
    assert_eq!(
        held.get("src/lib.rs"),
        Some(&Held::Changed),
        "every mutation of it is placed by a byte offset into the bytes the run read, and \
         drawing them onto what the file holds now would mark places nothing happened. \
         The file is not left out: a projection that lost a file without saying so would \
         be read as a run with nothing to say about it"
    );
}

#[test]
fn a_run_that_recorded_no_digest_is_believed_about_the_file() {
    let dir = tree("something else entirely\n");
    let held = read(&document(vec![mutant("src/lib.rs", "")]), dir.path()).expect("the file");
    assert_eq!(
        held.get("src/lib.rs"),
        Some(&Held::Measured("something else entirely\n".to_owned())),
        "a report from a release that recorded no digest cannot be checked, and refusing \
         every one of its files would make an older report unreadable rather than \
         unchecked"
    );
}

#[test]
fn a_file_the_report_names_and_the_tree_does_not_hold_is_a_refusal() {
    let dir = tree(SOURCE);
    let refused = read(&document(vec![mutant("src/gone.rs", "")]), dir.path())
        .expect_err("a file this tree does not hold");
    assert!(
        refused.to_string().contains("src/gone.rs"),
        "and it names the file: a projection quietly missing every mutation of one is \
         read as a run that had nothing to say about it: {refused}"
    );
}

#[test]
fn a_file_two_mutations_are_in_is_read_once() {
    let dir = tree(SOURCE);
    let digest = rust_mutants::id::digest(SOURCE.as_bytes());
    let held = read(
        &document(vec![
            mutant("src/lib.rs", &digest),
            mutant("src/lib.rs", "a digest that does not match"),
        ]),
        dir.path(),
    )
    .expect("the file the run measured");
    let two_files = read(
        &document(vec![mutant("src/lib.rs", ""), mutant("src/other.rs", "")]),
        dir.path(),
    );
    assert!(
        two_files.is_err(),
        "a report naming a second file this tree does not hold is refused on that file \
         and not on the first: stopping at the first would hand back a projection of one \
         file and call it the run"
    );

    assert_eq!(
        held.len(),
        1,
        "a file is read once however many mutations are in it: a catalog holds thousands \
         and a projection that read the file for each of them would read one file a \
         thousand times"
    );
    assert_eq!(
        held.get("src/lib.rs"),
        Some(&Held::Measured(SOURCE.to_owned())),
        "and the first mutation of it settles what it is, since they all name the bytes \
         one run read"
    );

    std::fs::write(dir.path().join("src/other.rs"), "pub fn other() {}\n").expect("a second file");
    let both = read(
        &document(vec![
            mutant("src/lib.rs", &digest),
            mutant("src/lib.rs", &digest),
            mutant("src/other.rs", ""),
        ]),
        dir.path(),
    )
    .expect("both files the run measured");
    assert_eq!(
        both.len(),
        2,
        "a file already read is passed over and the ones after it are still read: \
         stopping there would hand back a projection of the files before the first \
         repeat, and a catalog repeats on its second mutation"
    );
}

#[test]
fn a_path_this_shape_cannot_be_written_down_is_a_failure_and_not_a_ledger() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = read(&document(Vec::new()), dir.path()).expect("a report naming no file");
    assert!(
        held.is_empty(),
        "a report with no mutation in it names no file, and reading one would be reading \
         a file nobody said anything about"
    );
}
