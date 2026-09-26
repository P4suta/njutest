// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree whose lines end the other way: the same program, the same fates, and identities of its own.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest_devkit::fixture::{Fate, Fixture};
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/metadata.rs");

/// A copy of `fixture-simple` with every source line ending the other way.
fn crlf_copy() -> Fixture {
    let fixture = Fixture::copy("fixture-simple");
    for relative in rust_sources(fixture.root()) {
        let text = String::from_utf8(fixture.read(&relative)).expect("utf-8");
        fixture.write(
            &relative,
            rust_mutants::testkit::source::crlf(&text).as_bytes(),
        );
    }
    fixture
}

/// Every Rust file of a tree, by the path a report would name.
fn rust_sources(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("fixture directory entry");
            let path = entry.path();
            if test_metadata(&path).is_dir() {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(
                    njutest_devkit::paths::utf8(
                        path.strip_prefix(root)
                            .expect("a path walked under the fixture stays under its root"),
                    )
                    .replace('\\', "/"),
                );
            }
        }
    }
    found.sort();
    found
}

/// What one run of a tree establishes, and the full identity of every row.
fn run(fixture: &Fixture) -> (Vec<Fate>, Vec<String>) {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let newest = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
    .join("run-report-v1.json");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(newest).expect("the report"),
    )
    .expect("the report is a document");
    let rows = document["mutants"].as_array().expect("the rows");
    let mut fates: Vec<Fate> = rows
        .iter()
        .map(|row| Fate {
            path: row["path"].as_str().expect("a row path").to_owned(),
            line: u32::try_from(row["line"].as_u64().expect("a row line"))
                .expect("a row line fits its schema"),
            column: u32::try_from(row["column"].as_u64().expect("a row column"))
                .expect("a row column fits its schema"),
            rule: row["rule"].as_str().expect("a row rule").to_owned(),
            outcome: row["outcome"].as_str().expect("a row outcome").to_owned(),
        })
        .collect();
    fates.sort();
    let mut ids: Vec<String> = rows
        .iter()
        .map(|row| row["id"].as_str().expect("a row identity").to_owned())
        .collect();
    ids.sort();
    (fates, ids)
}

#[test]
fn a_crlf_tree_reaches_the_same_fates_at_the_same_places() {
    let (lf, lf_ids) = run(&Fixture::copy("fixture-simple"));
    let (crlf, crlf_ids) = run(&crlf_copy());
    assert_eq!(
        lf_ids.len(),
        crlf_ids.len(),
        "both trees mint one identity per fate"
    );
    assert_eq!(
        crlf, lf,
        "a file whose lines end the other way is the same program: the same mutations at the \
         same lines and columns, with the same fates"
    );
    assert!(!lf.is_empty(), "the fixture catalogs something");
}

#[test]
fn a_crlf_tree_mints_its_own_identities() {
    let (lf_fates, lf) = run(&Fixture::copy("fixture-simple"));
    let (crlf_fates, crlf) = run(&crlf_copy());
    assert_eq!(
        lf_fates, crlf_fates,
        "line endings do not change program fates"
    );
    assert_eq!(crlf.len(), lf.len(), "the same number of mutations");
    for id in &crlf {
        assert!(
            !lf.contains(id),
            "an identity hashes the exact bytes of the file it was cut from, so a tree whose \
             lines end the other way is a different tree and answers to different identities: \
             {id}"
        );
    }
}

#[test]
fn the_fates_a_crlf_tree_reaches_are_the_ones_its_readme_states() {
    let stated = njutest_devkit::fixture::stated_fates("fixture-simple");
    let (crlf, ids) = run(&crlf_copy());
    assert_eq!(crlf.len(), ids.len(), "every fate has one identity");
    assert_eq!(
        crlf, stated.rows,
        "the fates a fixture's README states are about the program, and the line endings are \
         not part of the program"
    );
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
