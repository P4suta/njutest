// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The three projections a team's existing surfaces read.

#![expect(
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking"
)]

use njutest::config::Contract;
use njutest::report::{
    BuildReport, Finding, FindingKind, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Position, Report, RunKind, TargetRecord, TargetStatus, html, junit, sarif,
};

/// One survivor of `src/lib.rs`, at a line the run knows or at none at all.
fn found(subject: &str, detail: &str, line: u32) -> Finding {
    let mut finding = Finding::new(FindingKind::SurvivingMutant, subject, detail);
    finding.path = Some("src/lib.rs".to_owned());
    finding.position = Some(Position {
        line,
        column: line,
        character_column: line,
    });
    finding
}

fn report_with(findings: Vec<Finding>) -> Report {
    report_varying(findings, &|_source| {})
}

/// The report of [`report_with`], with `vary` laid over its records and the findings they decide raised from them, as a run raises them.
fn report_varying(findings: Vec<Finding>, vary: &dyn Fn(&mut BuildReport)) -> Report {
    let mut source = BuildReport::new("fixture-evidence", RunKind::Full, Contract::StandardV1);
    source.repository.root_name = "workspace".to_owned();
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    source.toolchain.rustc = "rustc 1.98.0".to_owned();
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    source.timing.started = "2026-09-05T08:15:00Z".to_owned();
    source.timing.finished = "2026-09-05T08:15:01Z".to_owned();
    source.timing.duration_ms = 1500;
    source.targets = vec![
        TargetRecord {
            id: "a".to_owned(),
            name: "core/lib/core tests::works".to_owned(),
            package: "core".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: 40,
            message: None,
        },
        TargetRecord {
            id: "b".to_owned(),
            name: "core/lib/core tests::breaks".to_owned(),
            package: "core".to_owned(),
            status: TargetStatus::Failed,
            duration_ms: 10,
            message: Some("assertion failed: <left> != \"right\" & 'x'".to_owned()),
        },
        TargetRecord {
            id: "c".to_owned(),
            name: "core/lib/core tests::later".to_owned(),
            package: "core".to_owned(),
            status: TargetStatus::Skipped,
            duration_ms: 0,
            message: Some("libtest ignored it".to_owned()),
        },
    ];
    source.count_targets().expect("one exact target accounting");
    source.mutants = vec![MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "cccccccccccccccccccc".to_owned(),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 12,
            column: 9,
            character_column: 9,
        },
        rule: "lt-to-le@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: njutest::report::Decided::Survived,
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    }];
    source.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        survived: 1,
        observers: ObserverAccounting {
            unnoticed: 1,
            ..ObserverAccounting::default()
        },
        ..MutantAccounting::default()
    };
    source.findings = findings;
    vary(&mut source);
    njutest::testkit::raise_what_the_records_decide(&mut source);
    source.limitations = vec![
        Limitation::new(
            rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE,
            "doctests run once",
        ),
        Limitation::new(
            "git-metadata-unavailable",
            "the fixture is not a git repository",
        ),
    ];
    njutest::testkit::read_every_named_file(&mut source);
    source.verdict = source.concluded();
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let run =
        rust_mutants::id::RunId::try_from("20260905t081500z-abcdef").expect("a canonical run id");
    let latticed = njutest::report::across::configured(&run, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

fn report() -> Report {
    report_with(vec![
        found("cccccccccccccccccccc", "no test noticed <this> & that", 12),
        found(
            "dddddddd",
            "no measured target reaches mul-to-div@1 at src/lib.rs",
            0,
        ),
    ])
}

#[test]
fn the_page_is_self_contained() {
    let page = html::document(&report()).expect("the page renders");
    let outside = njutest_devkit::report::reaches_outside(&page);
    assert!(
        outside.is_empty(),
        "a page that fetches anything is a page that stops working offline, and a report \
         is read from a build artefact as often as from a desk: {outside:?}"
    );
    assert!(
        !page.contains("<script"),
        "and this one runs nothing of its own either: everything on it is a fact the run \
         established, laid out, so there is nothing for a script to do and nothing for \
         one to be wrong about"
    );
    assert!(page.starts_with("<!doctype html>"), "{page}");
    assert!(page.trim_end().ends_with("</html>"));
}

#[test]
fn the_page_leads_with_the_verdict_and_names_what_was_found() {
    let page = html::document(&report()).expect("the page renders");
    assert!(
        page.contains("<h1 class=\"not-assured\">Insufficient</h1>"),
        "{page}"
    );
    assert!(page.contains("cccccccc"), "{page}");
    assert!(page.contains("lt-to-le@1"), "{page}");
    assert!(
        page.contains(rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE),
        "{page}"
    );
}

#[test]
fn nothing_a_test_printed_can_become_markup() {
    let page = html::document(&report()).expect("the page renders");
    assert!(
        page.contains("&lt;left&gt;"),
        "a message from a test is text, not markup: {page}"
    );
    assert!(page.contains("&amp;"), "{page}");
    assert!(!page.contains("<left>"), "{page}");
}

#[test]
fn the_sarif_log_carries_every_finding_with_a_rule_and_a_place() {
    let log = sarif::document(&report()).expect("the sarif log derives");
    assert_eq!(log["version"], sarif::VERSION);
    let run = &log["runs"][0];
    assert_eq!(run["tool"]["driver"]["name"], "njutest");
    assert_eq!(run["automationDetails"]["id"], "20260905t081500z-abcdef");

    let results = run["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["ruleId"], "surviving-mutant");
    assert_eq!(
        results[0]["level"], "warning",
        "a gap in what was established is not a fault in the code"
    );
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
        12
    );
    let rules = run["tool"]["driver"]["rules"].as_array().expect("rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], "surviving-mutant");
}

#[test]
fn a_fault_in_the_code_is_an_error_and_a_gap_is_a_warning() {
    let report = report_with(vec![
        Finding::new(FindingKind::BuildFailure, "workspace", "mismatched types"),
        Finding::new(
            FindingKind::SurvivingMutant,
            "cccccccccccccccccccc",
            "nothing noticed",
        ),
    ]);
    let log = sarif::document(&report).expect("the sarif log derives");
    let levels: Vec<&str> = log["runs"][0]["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter_map(|result| result["level"].as_str())
        .collect();
    assert_eq!(levels, ["error", "warning"]);
}

#[test]
fn the_sarif_run_carries_the_verdict_and_the_accounting() {
    let log = sarif::document(&report()).expect("the sarif log derives");
    let properties = &log["runs"][0]["properties"];
    assert_eq!(properties["verdict"], "Insufficient");
    assert_eq!(properties["accounting"]["targets"]["selected"], 3);
    assert_eq!(
        properties["limitations"][0]["name"],
        rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE
    );
}

#[test]
fn the_junit_document_counts_what_a_reader_of_it_expects() {
    let document = junit::document(&report()).expect("the junit document renders");
    assert!(document.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(
        document.contains("tests=\"5\" failures=\"3\" skipped=\"1\""),
        "{document}"
    );
    assert!(
        document.contains("<skipped message=\"libtest ignored it\"/>"),
        "{document}"
    );
    assert!(document.contains("<failure message="), "{document}");
}

#[test]
fn a_finding_is_a_failing_case_so_it_shows_up_where_people_look() {
    let document = junit::document(&report()).expect("the junit document renders");
    assert!(document.contains("name=\"findings\""), "{document}");
    assert!(
        document.contains("classname=\"surviving-mutant\""),
        "{document}"
    );
}

#[test]
fn an_unmatched_acceptance_reaches_the_human_and_machine_projections() {
    let report = report_with(vec![
        Finding::new(
            FindingKind::SurvivingMutant,
            "cccccccccccccccccccc",
            "no test noticed the edit",
        ),
        Finding::new(
            FindingKind::UnmatchedAcceptance,
            "ffff",
            "no mutant matches this acceptance",
        ),
    ]);

    let page = html::document(&report).expect("the page renders");
    assert!(page.contains("unmatched-acceptance"), "{page}");
    assert!(page.contains("ffff"), "{page}");

    let log = sarif::document(&report).expect("the sarif log derives");
    assert_eq!(
        log["runs"][0]["results"][0]["ruleId"],
        "unmatched-acceptance"
    );
    assert_eq!(log["runs"][0]["results"][0]["level"], "warning");

    let cases = junit::document(&report).expect("the junit document renders");
    assert!(
        cases.contains("classname=\"unmatched-acceptance\""),
        "{cases}"
    );
}

#[test]
fn nothing_a_test_printed_can_close_a_tag() {
    let document = junit::document(&report()).expect("the junit document renders");
    assert!(document.contains("&lt;left&gt;"), "{document}");
    assert!(document.contains("&quot;right&quot;"), "{document}");
    assert!(document.contains("&apos;x&apos;"), "{document}");
}

#[test]
fn the_identity_a_reader_needs_travels_as_properties() {
    let document = junit::document(&report()).expect("the junit document renders");
    assert!(
        document.contains("<property name=\"run_id\" value=\"20260905t081500z-abcdef\"/>"),
        "{document}"
    );
    assert!(
        document.contains("<property name=\"mutants_survived\" value=\"1\"/>"),
        "{document}"
    );
}

#[test]
fn every_place_the_sarif_log_names_is_one_a_reader_of_the_repository_can_open() {
    let log = sarif::document(&report()).expect("the sarif log derives");
    let run = &log["runs"][0];
    let rules: Vec<&str> = run["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .filter_map(|rule| rule["id"].as_str())
        .collect();
    let measured = report()
        .conclusion()
        .expect("the checked report has a representable conclusion");
    let paths: Vec<&str> = measured
        .mutants
        .iter()
        .map(njutest::report::ProjectedMutant::path)
        .collect();

    for result in run["results"].as_array().expect("results") {
        assert!(
            result["ruleId"]
                .as_str()
                .is_some_and(|id| rules.contains(&id)),
            "a result whose rule the driver does not declare is one a consumer refuses \
             the whole log for: {result}"
        );
        let Some(location) = result["locations"].as_array().and_then(|all| all.first()) else {
            assert!(
                result["message"]["text"]
                    .as_str()
                    .is_some_and(|said| !said.is_empty()),
                "a finding with nowhere to point still says what it is, because a result \
                 with neither a place nor a sentence is a row a reader cannot act on: \
                 {result}"
            );
            continue;
        };
        let place = &location["physicalLocation"];
        let uri = place["artifactLocation"]["uri"]
            .as_str()
            .expect("a place a result is at");
        assert!(
            paths.contains(&uri),
            "a result points at {uri}, which is not a file this run measured. Code \
             scanning shows an alert against the path in the log, so a log that names \
             an identity instead of a file puts every finding on a file nobody has: \
             {result}"
        );
        assert!(
            !uri.starts_with('/') && !uri.contains(':'),
            "and it is relative to the repository, because an absolute path is a path \
             on the machine that ran it: {uri}"
        );
        assert!(
            place["region"]["startLine"]
                .as_u64()
                .is_some_and(|line| line >= 1),
            "and its line is one a file has. SARIF counts from one, so a region at line \
             zero is a log a consumer refuses, and a finding whose position the run does \
             not know is given no place rather than a place nobody can go to: {result}"
        );
    }
}

#[test]
fn a_moved_baseline_is_told_as_a_measurement_the_proofs_cannot_stand_on() {
    use njutest::report::drift::{Drift, Moved};
    let nothing = || Moved {
        gained: std::collections::BTreeSet::new(),
        lost: std::collections::BTreeSet::new(),
    };
    let report = report_varying(
        vec![found("cccccccccccccccccccc", "no test noticed it", 12)],
        &|source| {
            source.drift = vec![Drift::Moved {
                target: "core/lib/core tests::works".to_owned(),
                reached: Moved {
                    gained: std::collections::BTreeSet::from([3]),
                    lost: std::collections::BTreeSet::from([2]),
                },
                bodies: nothing(),
                infected: nothing(),
            }];
        },
    );
    let root = tempfile::tempdir().expect("a directory with no source in it");
    let sources = njutest::presentation::Sources::read(root.path(), &report).expect("sources");
    let told = njutest::presentation::Told::of(&report, &sources, "kept").expect("told");
    let said: Vec<&njutest::presentation::Diagnostic> = told
        .diagnostics
        .iter()
        .filter(|one| one.code == "NJ-UNSTABLE-BASELINE")
        .collect();
    let [diagnostic] = said.as_slice() else {
        panic!(
            "one moved target is one thing to be told: {:?}",
            told.diagnostics
        );
    };
    assert_eq!(
        diagnostic.title,
        "this test target reached different code on two runs of the same passing tests, so \
         every proof that removed a run because of what it reached is unfounded",
        "a reader told only that something was found would look for a missing assertion; the \
         thing to fix is a suite whose reach depends on something other than itself"
    );
    assert!(
        diagnostic.at.is_none(),
        "the finding is about a target, and no line of the source is where it is"
    );
    assert_eq!(
        diagnostic.notes,
        [
            "core/lib/core tests::works reached something on an original-code control that \
             it did not reach on its baseline, over the same passing tests, so what it \
             reaches is not a function of the target and every proof read off its baseline is \
             unfounded: 1 mutation a proof removed its run of, and 0 mutations no test \
             reached, rest on it. Make what the suite reaches independent of order, time and \
             earlier processes, and run again"
        ],
        "the sentence names what moved, what rests on it, and what to do"
    );
}

#[test]
fn a_knob_that_broke_a_target_is_told_as_a_suite_that_depends_on_its_machine() {
    use njutest::report::knobs::{Knob, KnobRecord, Standing};
    let broke = KnobRecord {
        target: "core/lib/core tests::works".to_owned(),
        knob: Knob::Timezone,
        standing: Standing::Broke {
            failed: vec!["the_zone_is_not_lord_howe".to_owned()],
        },
    };
    let report = report_varying(
        vec![found("cccccccccccccccccccc", "no test noticed it", 12)],
        &|source| source.knobs = vec![broke.clone()],
    );
    let root = tempfile::tempdir().expect("a directory with no source in it");
    let sources = njutest::presentation::Sources::read(root.path(), &report).expect("sources");
    let told = njutest::presentation::Told::of(&report, &sources, "kept").expect("told");
    let said: Vec<&njutest::presentation::Diagnostic> = told
        .diagnostics
        .iter()
        .filter(|one| one.code == "NJ-ENVIRONMENT")
        .collect();
    let [diagnostic] = said.as_slice() else {
        panic!(
            "one target a knob broke is one thing to be told: {:?}",
            told.diagnostics
        );
    };
    assert_eq!(
        diagnostic.severity,
        njutest::presentation::Severity::Refusal,
        "a suite whose answer depends on the machine is a defect, and is told as one"
    );
    assert_eq!(
        diagnostic.title,
        "this test target passes here and fails where something a machine may set \
         differently is set differently",
    );
    assert!(
        diagnostic.at.is_none(),
        "the finding is about a target, and no line of the source is where it is"
    );
    assert_eq!(
        diagnostic.notes,
        [
            "core/lib/core tests::works passed on its baseline and failed with \
             TZ=Australia/Lord_Howe: the_zone_is_not_lord_howe. What it answers depends on \
             something that differs between machines; set it in the test, or make the code not \
             read it, and run again"
        ],
        "the sentence names the knob, the value it was put to, the tests that failed, and what \
         to do"
    );
}
