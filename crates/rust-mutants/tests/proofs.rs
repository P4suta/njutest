// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The branch proofs a real workspace earns, and the ones it does not.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::PrepareOptions;
use rust_mutants::workspace::{OpenOptions, Workspace};

struct Fixture {
    root: PathBuf,
    temp: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-proofs-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");
    Fixture {
        root,
        temp,
        _dir: dir,
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

#[test]
fn the_compiler_vouches_for_a_condition_of_primitives_and_refuses_the_rest() {
    let fixture = fixture("fixture-coverage");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp.clone(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");

    let proven: Vec<String> = session
        .accepted()
        .iter()
        .filter(|index| session.branch(**index).is_some())
        .filter_map(|index| session.catalog().by_index(*index))
        .map(|mutant| {
            format!(
                "{} {}",
                mutant.candidate.rule.name,
                session.position(mutant).map_or(0, |position| position.line)
            )
        })
        .collect();
    assert!(
        proven.iter().any(|one| one.starts_with("le-to-lt 8")),
        "a condition of primitives earns its proof: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 16")),
        "a comparison the compiler will not vouch for earns none: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 28")),
        "a condition that runs the program's code earns none: {proven:?}"
    );
    assert!(session.proven() > 0, "{proven:?}");

    let proof = session
        .accepted()
        .iter()
        .find_map(|index| session.branch(*index))
        .expect("a proof");
    assert!(
        proof.body_start.line < proof.body_end.line,
        "the proof names the body it gates: {proof:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_witnessed_tree_is_put_back_before_anything_is_instrumented() {
    let fixture = fixture("fixture-coverage");
    let before = std::fs::read_to_string(fixture.root.join("src/lib.rs")).expect("the source");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp.clone(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("src/lib.rs")).expect("the source"),
        before,
        "the source tree is read-only, whatever the engine writes into its own copy"
    );
    let instrumented =
        std::fs::read_to_string(session.snapshot_root().join("src/lib.rs")).expect("the copy");
    assert!(
        !instrumented.contains("rust-mutants-witness-v1"),
        "the witness tree is taken out before anything is instrumented"
    );
    assert!(
        instrumented.contains("::active("),
        "and what is left is the instrumented tree"
    );
    session.close().expect("the session closes");
}
