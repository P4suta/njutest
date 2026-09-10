// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The three projections a team's existing surfaces read, put to their readers here rather than through a process.
//!
//! A run report is what a program reads and these are what somebody else's
//! program reads, so each is held to the shape its reader accepts. Driving the
//! command instead means starting a process, and a measurement of what a
//! crate's own tests reach does not follow a guard across that boundary.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reads a document by the names its own fixture put there"
)]

use std::collections::BTreeMap;

use rust_mutants_cli::report::run::{
    Accounting, FindingDocument, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::report::sources::Held;
use rust_mutants_cli::report::stryker::Thresholds;
use rust_mutants_cli::report::{
    PlatformDocument, SelectionDocument, WorkspaceDocument, html, sarif, stryker,
};

/// The source every mutation in the fixture is in.
const SOURCE: &str = "pub fn wide(n: i32) -> bool {\n    n > 1\n}\n";

/// A name a reader must not be able to close a tag with.
const MARKUP: &str = "<script>alert('x')</script>";

fn mutant(index: u32, outcome: &str, replacement: &str) -> RunMutantDocument {
    RunMutantDocument {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
        rule_version: 1,
        line: 2,
        column: 7,
        start_byte: 35,
        end_byte: 36,
        source_digest: format!("{index:064x}"),
        original: ">".to_owned(),
        replacement: replacement.to_owned(),
        outcome: outcome.to_owned(),
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
        tests_run: Some(1),
        killed_by: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        identical: None,
        retried: false,
        expected: false,
        unreached: false,
        source_run_id: None,
    }
}

fn document() -> RunDocument {
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 1,
        tool_version: "0.1.0".to_owned(),
        run: RunMeta {
            id: "20260905T120000000Z".to_owned(),
            started_at: "2026-09-05T12:00:00Z".to_owned(),
            finished_at: "2026-09-05T12:00:01Z".to_owned(),
            duration_ms: 1000,
            interrupted: false,
            exit_code: 1,
            shard: None,
        },
        workspace: WorkspaceDocument {
            root_name: MARKUP.to_owned(),
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
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: Accounting {
            cataloged: 2,
            refused: 0,
            skipped: 0,
            executed: 2,
            killed: 1,
            survived: 1,
            timed_out: 0,
            inconclusive: 0,
            errored: 0,
            unreached: 0,
            discharged: 0,
            not_run: 0,
            expected: 0,
        },
        score: Some(ScoreDocument {
            detected: 1,
            decided: 2,
            value: 0.5,
        }),
        mutants: vec![mutant(0, "killed", ">="), mutant(1, "survived", "<")],
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: Vec::new(),
        findings: vec![FindingDocument {
            kind: "surviving-mutant".to_owned(),
            mutant: Some(format!("{:064x}", 1)),
            detail: "no test noticed this".to_owned(),
        }],
    }
}

fn sources() -> BTreeMap<String, Held> {
    BTreeMap::from([("src/lib.rs".to_owned(), Held::Measured(SOURCE.to_owned()))])
}

#[test]
fn the_page_needs_nothing_from_the_network_and_closes_no_tag_it_was_given() {
    let page = html::document(&document(), &sources());
    let outside = mjutest_devkit::report::reaches_outside(&page);
    assert!(
        outside.is_empty(),
        "a run report is read from a build artefact on a machine with no network as \
         often as from a desk: {outside:?}"
    );
    assert!(
        page.starts_with("<!doctype html>") && page.trim_end().ends_with("</html>"),
        "and it is one whole document rather than a fragment somebody has to wrap"
    );
    assert!(
        !page.contains(MARKUP) && page.contains("&lt;script&gt;"),
        "while a name the run read off a tree cannot close a tag it was put inside: a \
         workspace is called whatever somebody called it, and a page that pastes that in \
         is a page whose shape the tree under test decides"
    );
}

#[test]
fn the_stryker_projection_is_the_shape_that_reader_accepts() {
    let projection = stryker::project(
        &document(),
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &sources(),
    )
    .expect("every mutated file is one the run measured");
    let json = serde_json::to_value(&projection).expect("the projection is a document");
    assert_eq!(
        json["schemaVersion"], "2.0",
        "the reader is told which shape this is before it reads any of it: {json}"
    );
    let file = &json["files"]["src/lib.rs"];
    assert_eq!(
        file["source"], SOURCE,
        "a mutated file carries the source the run measured, because the reader draws \
         every mutation onto it and a file it does not have is a file it draws nothing \
         on: {json}"
    );
    let mutants = file["mutants"].as_array().expect("the mutations");
    assert_eq!(mutants.len(), 2, "{json}");
    assert_eq!(
        (mutants[0]["status"].as_str(), mutants[1]["status"].as_str()),
        (Some("Killed"), Some("Survived")),
        "and each says what happened to it in the words that reader knows, which are \
         not the words this one uses: {json}"
    );
    assert_eq!(
        mutants[0]["location"]["start"]["line"].as_u64(),
        Some(2),
        "a location is where the mutation is, counted the way the reader counts: {json}"
    );

    let moved = stryker::project(
        &document(),
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &BTreeMap::from([("src/lib.rs".to_owned(), Held::Changed)]),
    );
    assert!(
        moved.is_err(),
        "while a file that is no longer the one the run measured is refused rather than \
         drawn on: every mutation would be marked at a byte that means something else \
         now, and the reader would show a person a place nothing happened"
    );
}

/// One finding, about `mutant` or about nothing.
fn found(kind: &str, mutant: Option<&str>) -> FindingDocument {
    FindingDocument {
        kind: kind.to_owned(),
        mutant: mutant.map(ToOwned::to_owned),
        detail: format!("what {kind} means here"),
    }
}

#[test]
fn a_sarif_alert_carries_the_level_its_kind_earns_and_the_place_it_is_about() {
    let mut document = document();
    document.findings = vec![
        found("surviving-mutant", Some(&format!("{:064x}", 1))),
        found("unreached-mutant", Some(&format!("{:064x}", 0))),
        found("build-failure", None),
        found("surviving-mutant", Some("a mutation this run never judged")),
    ];
    let json = serde_json::to_value(sarif::log(&document)).expect("the log is a document");
    let results = json["runs"][0]["results"]
        .as_array()
        .expect("the results")
        .clone();
    assert_eq!(
        results.len(),
        4,
        "every finding is one alert, including the one about no mutation and the one \
         naming a mutation this run never judged: a finding a log drops is a finding the \
         surface a team reads never shows: {json}"
    );
    assert_eq!(
        results
            .iter()
            .map(|one| one["level"].as_str().unwrap_or_default())
            .collect::<Vec<&str>>(),
        vec!["warning", "note", "error", "warning"],
        "and each carries the level its kind earns: a gap in the tests is a warning, \
         something nobody measured is a note, and a fault in the code under test is an \
         error. A log that called them all one thing would be one a team filters by \
         nothing: {json}"
    );
    assert!(
        results[2]["locations"]
            .as_array()
            .is_some_and(Vec::is_empty)
            && results[3]["locations"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "a finding about no place carries no place, rather than one the reader would \
         open: {json}"
    );
    assert_eq!(
        results[0]["partialFingerprints"]["rustMutantsMutantId/v1"].as_str(),
        Some(format!("{:064x}", 1).as_str()),
        "an alert about a mutation carries that mutation's identity under the name the \
         reader groups by, or every run of the same gap is a new alert somebody has to \
         triage again: {json}"
    );
    assert_eq!(
        json["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "and one descriptor per rule, however many findings that rule accounts for: {json}"
    );
}

#[test]
fn a_region_ends_where_the_original_does_when_the_original_is_one_line() {
    let mut document = document();
    document.mutants[0].original = ">".to_owned();
    document.findings = vec![found("surviving-mutant", Some(&format!("{:064x}", 0)))];
    let json = serde_json::to_value(sarif::log(&document)).expect("the log is a document");
    let region =
        json["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"].clone();
    assert_eq!(
        (
            region["startColumn"].as_u64(),
            region["endColumn"].as_u64(),
            region["snippet"]["text"].as_str()
        ),
        (Some(7), Some(8), Some(">")),
        "a region ends one column past what the mutation replaced, and carries it, so \
         the surface underlines the code and not the line: {region}"
    );

    let mut spanning = document.clone();
    spanning.mutants[0].original = "if a {\n    b\n}".to_owned();
    let json = serde_json::to_value(sarif::log(&spanning)).expect("the log is a document");
    let region =
        json["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"].clone();
    assert!(
        region["endColumn"].is_null(),
        "while one whose original runs over more than a line has no end column on that \
         line: counting its bytes would underline into the middle of the next one: \
         {region}"
    );
}

#[test]
fn the_sarif_log_carries_every_finding_where_a_reader_can_open_it() {
    let log = sarif::log(&document());
    let json = serde_json::to_value(&log).expect("the log is a document");
    assert_eq!(json["version"], sarif::VERSION);
    let results = json["runs"][0]["results"]
        .as_array()
        .expect("the results")
        .clone();
    assert_eq!(results.len(), 1, "one finding, one result: {json}");
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].as_str(),
        Some("src/lib.rs"),
        "an alert is on the file the mutation is in: a log that gave the identity \
         instead puts every alert on a path nobody has: {json}"
    );
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["region"]["startLine"].as_u64(),
        Some(2),
        "and on the line it is on: {json}"
    );
}
