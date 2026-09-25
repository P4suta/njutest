// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The markers an author writes, end to end: what each hides, and the one that hides nothing.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document by the names its own fixture put there"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn run(fixture: &Fixture, extra: &[&str]) -> std::process::Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all"])
            .chain(["--offline", "--locked", "--no-coverage"])
            .chain(extra.iter().copied())
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let directory = rust_mutants_cli::app::stored::Store::read(fixture.root()).root();
    let pointer: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(directory.join("latest.json"))
            .expect("a pointer to the newest run"),
    )
    .expect("the pointer is a document");
    let relative = pointer["document"].as_str().expect("a document path");
    let text = std::fs::read_to_string(directory.join(relative)).expect("the report");
    njutest_devkit::strictjson::decode_str(&text).expect("JSON")
}

#[test]
fn every_marker_hides_what_it_says_and_the_one_that_hides_nothing_is_a_finding() {
    let fixture = Fixture::copy("fixture-annotated");
    let output = run(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a marker that hides nothing is a finding: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(
        report["accounting"]["cataloged"].as_u64(),
        Some(4),
        "only what no marker covers is cataloged: {}",
        report["accounting"]
    );
    assert_eq!(
        report["accounting"]["killed"].as_u64(),
        Some(4),
        "{}",
        report["accounting"]
    );
    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["kind"], "unmatched-skip");
    assert!(
        findings[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("nothing starts here")),
        "the finding names the marker and quotes its reason: {}",
        findings[0]["detail"]
    );
    let annotated = report["skips"]
        .as_array()
        .expect("skips")
        .iter()
        .find(|skip| skip["reason"] == "annotated")
        .expect("the annotated tally");
    assert_eq!(
        annotated["count"].as_u64(),
        Some(10),
        "the tally says how much the markers hid: {annotated}"
    );
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        ci: rust_mutants_cli::CiHost::None,
    }
}

#[test]
fn a_skip_that_names_its_text_hides_nothing_once_the_text_has_moved_and_says_where_it_went() {
    let fixture = Fixture::copy("fixture-simple");
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        "version = 1\n[[mutation.skip]]\npath = \"src/lib.rs\"\nlines = \"11-11\"\ntext = \"if a > b\"\nreason = \"the comparison is the fixture's own\"\n",
    )
    .expect("the configuration");
    let held = run(&fixture, &[]);
    let report = document(&fixture);
    let findings = report["findings"].as_array().expect("findings");
    assert!(
        findings.iter().all(|one| one["kind"] != "unmatched-skip"),
        "the text is on the lines the entry names, so it hides what starts there: {findings:?} {}",
        njutest_devkit::process::strict_utf8(&held.stderr)
    );
    let configured = |report: &serde_json::Value| {
        report["skips"]
            .as_array()
            .expect("skips")
            .iter()
            .filter(|skip| skip["reason"] == "configured")
            .filter_map(|skip| skip["count"].as_u64())
            .sum::<u64>()
    };
    assert!(configured(&report) > 0, "{}", report["skips"]);
    let lib = fixture.root().join("src/lib.rs");
    let text = std::fs::read_to_string(&lib).expect("the library");
    std::fs::write(&lib, format!("// a line above everything\n{text}")).expect("the edit");
    let moved = run(&fixture, &[]);
    let report = document(&fixture);
    let findings = report["findings"].as_array().expect("findings");
    let unmatched: Vec<&serde_json::Value> = findings
        .iter()
        .filter(|one| one["kind"] == "unmatched-skip")
        .collect();
    assert_eq!(
        unmatched.len(),
        1,
        "line 11 now holds other code, and an entry whose text is not on its lines hides \
         nothing rather than whatever moved in: {findings:?} {}",
        njutest_devkit::process::strict_utf8(&moved.stderr)
    );
    assert!(
        unmatched[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("src/lib.rs:12")),
        "the finding says where the text went, so the entry is fixed by reading it: {}",
        unmatched[0]["detail"]
    );
    assert_eq!(configured(&report), 0, "{}", report["skips"]);
}
