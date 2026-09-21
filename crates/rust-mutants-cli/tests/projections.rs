// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The three projections a team's existing surfaces read, put to their readers here rather than through a process.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that build one document are not themselves tests, and a test \
              reads a document by the names its own fixture put there"
)]

use std::collections::BTreeMap;

use rust_mutants::outcome::Outcome;
use rust_mutants::report::catalog::{
    PlatformDocument, RejectionDocument, SelectionDocument, SkipDocument, WorkspaceDocument,
};
use rust_mutants::run::{FindingKind, NotRunReason};
use rust_mutants_cli::report::run::{
    Accounting, FindingDocument, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::report::sources::Held;
use rust_mutants_cli::report::stryker::Thresholds;
use rust_mutants_cli::report::{html, markdown, sarif, stryker, tally};

/// The source every mutation in the fixture is in.
const SOURCE: &str = "pub fn wide(n: i32) -> bool {\n    n > 1\n}\n";

/// A name a reader must not be able to close a tag with.
const MARKUP: &str = "<script>alert('x')</script>";

fn mutant(index: u32, outcome: Outcome, replacement: &str) -> RunMutantDocument {
    RunMutantDocument {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
        item: "demo".to_owned(),
        rule_version: 1,
        line: 2,
        column: 7,
        start_byte: 35,
        end_byte: 36,
        source_digest: format!("{index:064x}"),
        original: ">".to_owned(),
        replacement: replacement.to_owned(),
        outcome,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
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

fn document() -> RunDocument {
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 2,
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
            mutant_steps: None,
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: Accounting {
            cataloged: 2,
            refused: 0_u32.into(),
            skipped: 0_u32.into(),
            executed: 2_u32.into(),
            killed: 1_u32.into(),
            survived: 1_u32.into(),
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
            decided: 2,
            value: 0.5,
        }),
        mutants: vec![
            mutant(0, Outcome::Killed, ">="),
            mutant(1, Outcome::Survived, "<"),
        ],
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: Vec::new(),
        findings: vec![FindingDocument {
            kind: FindingKind::SurvivingMutant,
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
    let outside = njutest_devkit::report::reaches_outside(&page);
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
fn found(kind: FindingKind, mutant: Option<&str>) -> FindingDocument {
    FindingDocument {
        kind,
        mutant: mutant.map(ToOwned::to_owned),
        detail: format!("what {} means here", kind.as_str()),
    }
}

#[test]
fn a_sarif_alert_carries_the_level_its_kind_earns_and_the_place_it_is_about() {
    let mut document = document();
    document.findings = vec![
        found(FindingKind::SurvivingMutant, Some(&format!("{:064x}", 1))),
        found(FindingKind::UnreachedMutant, Some(&format!("{:064x}", 0))),
        found(FindingKind::ErroredMutant, None),
        found(
            FindingKind::SurvivingMutant,
            Some("a mutation this run never judged"),
        ),
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
        results[0]["partialFingerprints"]["rustMutantsMutation/v2"].as_str(),
        Some("src/lib.rs:demo:gt-to-ge:>"),
        "an alert about a mutation carries the place it is in under the name the reader \
         groups by — not the file's bytes, which the next commit re-mints, closing every \
         alert in the file and reopening it with the dismissals gone. Otherwise every \
         run of the same gap is a new alert somebody has to \
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
    document.findings = vec![found(
        FindingKind::SurvivingMutant,
        Some(&format!("{:064x}", 0)),
    )];
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

/// One mutant with the outcome and the reach a run gave it.
fn outcome(index: u32, outcome: Outcome, unreached: bool) -> RunMutantDocument {
    RunMutantDocument {
        unreached,
        not_run_reason: unreached.then_some(NotRunReason::Unreached),
        ..mutant(index, outcome, ">=")
    }
}

/// What the stryker projection made of a document holding just these mutants.
fn projected(mutants: Vec<RunMutantDocument>) -> serde_json::Value {
    let mut document = document();
    document.mutants = mutants;
    let projection = stryker::project(
        &document,
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &sources(),
    )
    .expect("every mutated file is one the run measured");
    serde_json::to_value(&projection).expect("the projection is a document")
}

#[test]
fn every_outcome_is_said_in_the_word_that_reader_knows() {
    let json = projected(vec![
        outcome(0, Outcome::Killed, false),
        outcome(1, Outcome::Survived, false),
        outcome(2, Outcome::StepLimitReached, false),
        outcome(3, Outcome::Waited, false),
        outcome(4, Outcome::Inconclusive, false),
        outcome(5, Outcome::Errored, false),
        outcome(6, Outcome::NotRun, true),
        outcome(7, Outcome::NotRun, false),
    ]);
    let mutants = json["files"]["src/lib.rs"]["mutants"]
        .as_array()
        .expect("the mutations")
        .clone();
    assert_eq!(
        mutants
            .iter()
            .map(|one| one["status"].as_str().unwrap_or_default())
            .collect::<Vec<&str>>(),
        vec![
            "Killed",
            "Survived",
            "Pending",
            "Pending",
            "RuntimeError",
            "RuntimeError",
            "NoCoverage",
            "Pending",
        ],
        "this run's words and that reader's are two vocabularies, and every one of ours \
         has to arrive as one of theirs: a status the reader does not know is a mutation \
         it draws nothing for. A mutation nothing reached is `NoCoverage` and not \
         `Survived`, because the two are different things to do about: {json}"
    );
    assert!(
        mutants[4]["statusReason"]
            .as_str()
            .is_some_and(|it| it.contains("did not reproduce"))
            && mutants[5]["statusReason"]
                .as_str()
                .is_some_and(|it| it.contains("exit")),
        "and the two that arrive as one word carry the sentence that parts them: a \
         timeout nobody could reproduce is not a harness that failed: {json}"
    );
    assert!(
        mutants[0]["statusReason"].is_null(),
        "while one whose status says everything carries no sentence: {json}"
    );
}

#[test]
fn only_a_mutation_something_noticed_names_what_noticed_it() {
    let mut named = outcome(0, Outcome::Killed, false);
    named.killed_by = vec!["demo::works".to_owned()];
    let mut unnamed = outcome(1, Outcome::Killed, false);
    unnamed.killed_by = Vec::new();
    let mut nowhere = outcome(2, Outcome::Killed, false);
    nowhere.killed_by = Vec::new();
    nowhere.target = String::new();
    let alive = outcome(3, Outcome::Survived, false);

    let json = projected(vec![named, unnamed, nowhere, alive]);
    let mutants = json["files"]["src/lib.rs"]["mutants"]
        .as_array()
        .expect("the mutations")
        .clone();
    assert_eq!(
        mutants[0]["killedBy"][0].as_str(),
        Some("demo::works"),
        "a harness that named the test that noticed is quoted: {json}"
    );
    assert_eq!(
        mutants[1]["killedBy"][0].as_str(),
        Some("demo/lib/demo"),
        "one that did not leaves the binary, which is the smallest true thing this run \
         can say about what noticed it: {json}"
    );
    assert!(
        mutants[2]["killedBy"].is_null() && mutants[3]["killedBy"].is_null(),
        "and where there is nothing to name, nothing is named: a survivor with a killer \
         beside it is a report saying two things: {json}"
    );
}

#[test]
fn a_column_is_counted_the_way_that_reader_counts_it() {
    let mut document = document();
    document.mutants = vec![mutant(0, Outcome::Killed, ">=")];
    let wide = "pub fn wide(n: i32) -> bool {\n    // ★★★ n > 1\n}\n";
    document.mutants[0].line = 2;
    document.mutants[0].column = 20;
    document.mutants[0].original = ">".to_owned();
    let projection = stryker::project(
        &document,
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &BTreeMap::from([("src/lib.rs".to_owned(), Held::Measured(wide.to_owned()))]),
    )
    .expect("the file the run measured");
    let json = serde_json::to_value(&projection).expect("the projection is a document");
    let start = json["files"]["src/lib.rs"]["mutants"][0]["location"]["start"].clone();
    assert_eq!(
        (start["line"].as_u64(), start["column"].as_u64()),
        (Some(2), Some(14)),
        "this schema counts columns in UTF-16 and a run counts them in bytes, so a line \
         with anything but ASCII before the mutation arrives at a different number: \
         three stars are nine bytes and three units, so byte twenty is unit fourteen, \
         and a reader handed the byte column underlines six columns to the right of the \
         code: {start}"
    );
    let end = json["files"]["src/lib.rs"]["mutants"][0]["location"]["end"].clone();
    assert_eq!(
        end["column"].as_u64(),
        Some(15),
        "and the end is one unit past what was replaced, in the same counting: {end}"
    );
}

#[test]
fn a_mutation_over_several_lines_ends_on_the_last_of_them() {
    let mut document = document();
    document.mutants = vec![mutant(0, Outcome::Killed, "")];
    document.mutants[0].line = 1;
    document.mutants[0].column = 1;
    document.mutants[0].original = "if a {\n    b\n}".to_owned();
    let projection = stryker::project(
        &document,
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &sources(),
    )
    .expect("the file the run measured");
    let json = serde_json::to_value(&projection).expect("the projection is a document");
    let location = json["files"]["src/lib.rs"]["mutants"][0]["location"].clone();
    assert_eq!(
        (
            location["start"]["line"].as_u64(),
            location["end"]["line"].as_u64(),
            location["end"]["column"].as_u64()
        ),
        (Some(1), Some(3), Some(2)),
        "a mutation that replaced three lines ends on the third of them, one past its \
         last unit: ending it on the first would underline a line and a bit of it, and \
         the reader would show the wrong code: {location}"
    );
    assert!(
        json["files"]["src/lib.rs"]["mutants"][0]["replacement"].is_null(),
        "and a mutation that replaced its bytes with nothing carries no replacement, \
         rather than an empty one the reader would draw as a change to nothing: {json}"
    );
}

#[test]
fn the_page_says_what_the_run_decided_and_what_it_decided_nothing_about() {
    let page = html::document(&document(), &sources());
    assert!(
        page.contains("50.0%") && page.contains("1 detected of 2 decided"),
        "a score is the first thing on the page, with the two numbers it came from \
         beside it: a percentage nobody can check is the one thing this program does not \
         report: {page}"
    );
    assert!(
        page.contains("2 mutants were cataloged"),
        "the total says what it is the total of, in a sentence rather than as a row a \
         reader would add to the outcomes below it: {page}"
    );
    for (name, count) in [("killed", 1), ("survived", 1)] {
        assert!(
            page.contains(&format!("<th>{name}</th><td>{count}</td>")),
            "and the outcomes are the rows, so a reader adding the table up gets the \
             catalog: {name} is not {count} in\n{page}"
        );
    }
    assert!(
        page.contains("add to the 2 cataloged"),
        "and the page says so, because a subset printed as a peer is a table that does \
         not add up: {page}"
    );
    assert!(
        page.contains("surviving-mutant"),
        "the findings are named: a page with a score and no findings is one a reader \
         takes as a clean run: {page}"
    );
    assert!(
        page.contains("pub fn wide"),
        "and the source the run measured is on it, so a reader sees the mutation where \
         it is rather than a line number to go and look up: {page}"
    );

    let mut nothing = document();
    nothing.score = None;
    nothing.mutants = Vec::new();
    nothing.findings = Vec::new();
    let page = html::document(&nothing, &sources());
    assert!(
        page.contains("decided nothing, which is not a score of zero"),
        "a run that decided nothing says so: nought per cent is what a suite that \
         noticed none of them earns, and a run that judged none of them earned nothing \
         at all: {page}"
    );
    assert!(
        page.contains("Nothing was found.") && page.contains("Nothing was cataloged."),
        "and the empty sections say they are empty rather than being absent, because a \
         section that is not there reads as one the release does not have: {page}"
    );
}

#[test]
fn every_row_carries_the_place_it_takes_when_a_reader_asks_for_findings_first() {
    let mut document = document();
    let mut accepted = outcome(4, Outcome::Survived, false);
    accepted.expected = true;
    document.mutants = vec![
        outcome(0, Outcome::Killed, false),
        outcome(1, Outcome::Survived, false),
        outcome(2, Outcome::NotRun, true),
        outcome(3, Outcome::Errored, false),
        accepted,
    ];
    let page = html::document(&document, &sources());
    let ranked: Vec<&str> = page
        .lines()
        .filter_map(|line| line.split("data-rank=\"").nth(1))
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(
        ranked,
        vec!["4", "0", "2", "1", "3"],
        "the rows a person has to act on carry the lowest place — a survivor nobody \
         accepted first, then what the run could not decide, then what nothing reached — \
         and the ones something noticed carry the highest. A survivor a reviewer accepted \
         goes below what nobody has looked at, because it is not a row anybody has to \
         act on. Without this the four rows that matter sit under the four hundred that \
         do not: {page}"
    );
}

/// Where one recorded projection lives.
fn recorded(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

#[test]
fn the_page_is_the_page_that_was_reviewed() {
    let page = html::document(&document(), &sources());
    njutest_devkit::golden::golden(&recorded("report.golden.html"), page.as_bytes())
        .expect("the page a reviewer read");
}

#[test]
fn the_stryker_projection_is_the_document_that_was_reviewed() {
    let projection = stryker::project(
        &document(),
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &sources(),
    )
    .expect("every mutated file is one the run measured");
    let text = serde_json::to_string_pretty(&projection).expect("the projection is a document");
    njutest_devkit::golden::golden(
        &recorded("stryker.golden.json"),
        format!("{text}\n").as_bytes(),
    )
    .expect("the projection a reviewer read");
}

#[test]
fn the_sarif_log_is_the_document_that_was_reviewed() {
    let text =
        serde_json::to_string_pretty(&sarif::log(&document())).expect("the log is a document");
    njutest_devkit::golden::golden(
        &recorded("sarif.golden.json"),
        format!("{text}\n").as_bytes(),
    )
    .expect("the log a reviewer read");
}

/// A run whose report carries every side of itself: what nothing reached, what was refused, what discovery passed over, a file the tests noticed every mutation in, and text with the characters a page has to escape.
fn everything() -> RunDocument {
    let mut document = document();
    let mut unreached = outcome(2, Outcome::NotRun, true);
    "src/other.rs".clone_into(&mut unreached.path);
    let mut not_run = outcome(3, Outcome::NotRun, false);
    "src/other.rs".clone_into(&mut not_run.path);
    let mut clean = outcome(4, Outcome::Killed, false);
    "src/clean.rs".clone_into(&mut clean.path);
    document.mutants.extend([unreached, not_run, clean]);
    document.rejections = vec![RejectionDocument {
        index: 9,
        id: format!("{:064x}", 9),
        display_id: format!("{:020x}", 9),
        path: "src/lib.rs".to_owned(),
        rule: "gt-to-ge".to_owned(),
        code: Some("E0308".to_owned()),
        diagnostic: "expected `bool`, found `&str` in \"wide\" & elsewhere".to_owned(),
        isolated: true,
    }];
    document.skips = vec![SkipDocument {
        reason: "macro-invocation".to_owned(),
        path: "src/lib.rs".to_owned(),
        count: 3,
        explanation: "what a macro expands to is decided during the build".to_owned(),
    }];
    document
}

fn everything_sources() -> BTreeMap<String, Held> {
    BTreeMap::from([
        ("src/lib.rs".to_owned(), Held::Measured(SOURCE.to_owned())),
        ("src/other.rs".to_owned(), Held::Changed),
        (
            "src/clean.rs".to_owned(),
            Held::Measured("pub fn clean() {}\n".to_owned()),
        ),
    ])
}

#[test]
fn the_page_of_a_run_with_every_side_to_it_is_the_page_that_was_reviewed() {
    let page = html::document(&everything(), &everything_sources());
    njutest_devkit::golden::golden(&recorded("report-everything.golden.html"), page.as_bytes())
        .expect("the page a reviewer read");
}

#[test]
fn the_page_of_a_run_that_cataloged_nothing_says_so_in_every_section() {
    let mut nothing = document();
    nothing.score = None;
    nothing.mutants = Vec::new();
    nothing.findings = Vec::new();
    let page = html::document(&nothing, &BTreeMap::new());
    njutest_devkit::golden::golden(&recorded("report-nothing.golden.html"), page.as_bytes())
        .expect("the page a reviewer read");
}

#[test]
fn the_stryker_projection_of_a_run_with_every_side_to_it_is_the_one_reviewed() {
    let mut document = everything();
    document
        .mutants
        .retain(|mutant| mutant.path == "src/lib.rs" || mutant.path == "src/clean.rs");
    let projection = stryker::project(
        &document,
        std::path::Path::new("."),
        Thresholds { high: 80, low: 60 },
        &everything_sources(),
    )
    .expect("every mutated file is one the run measured");
    let text = serde_json::to_string_pretty(&projection).expect("the projection is a document");
    njutest_devkit::golden::golden(
        &recorded("stryker-everything.golden.json"),
        format!("{text}\n").as_bytes(),
    )
    .expect("the projection a reviewer read");
}

/// Every projection of one run lays the accounting out from the same arrangement.
///
/// Four of them used to lay it out each for itself, and all four made the same
/// mistake — a subset printed beside the count it is part of — while two also
/// disagreed about which columns exist. The arrangement is one value now, and
/// this holds them to it: a projection that reaches past `Tally` to the raw
/// counts is one that can drift again.
#[test]
fn every_projection_lays_the_accounting_out_from_the_one_arrangement() {
    let document = document();
    let tally = tally::Tally::of(&document);
    let page = html::document(&document, &sources());
    let paged = markdown::document(&document);
    let said = rust_mutants_cli::report::lines(&document).expect("valid work ledger");

    for (name, count) in &tally.parts {
        for (projection, text) in [("html", &page), ("markdown", &paged)] {
            assert!(
                text.contains(&format!("{name}</th><td>{count}"))
                    || text.contains(&format!("| {name} | {count} |")),
                "{projection} does not carry the {name} column the arrangement holds: {text}"
            );
        }
        assert!(
            said.contains(&format!("{}={count}", name.replace(' ', "_"))),
            "the lines do not carry the {name} column the arrangement holds: {said}"
        );
    }
    for (projection, text) in [("html", &page), ("markdown", &paged), ("lines", &said)] {
        for (_part, what, count) in &tally.within {
            assert!(
                !text.contains(&format!("{what}</th><td>{count}"))
                    && !text.contains(&format!("| {what} | {count} |")),
                "{projection} prints {what} as a row beside the counts it is part of, which \
                 is a table that does not add up: {text}"
            );
        }
    }
}
