// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The canonical projection of a report, and the schema that publishes it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use njutest_cli::report::{
    Accounting, Finding, FindingKind, Git, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Position, Report, Repository, RunKind, SCHEMA, Scope, SoundnessAccounting,
    TargetAccounting, TargetRecord, TargetStatus, Timing, Tool, Toolchain, Verdict, json,
};

/// A report with something in every field, so the schema is exercised whole.
fn populated() -> Report {
    Report {
        schema: SCHEMA.to_owned(),
        schema_version: njutest_cli::report::SCHEMA_VERSION,
        run_id: "20260905T081500Z-abcdef".to_owned(),
        provenance: njutest_cli::report::Provenance {
            identity: "f".repeat(64),
            facts: njutest_cli::report::Established::Here,
        },
        run_kind: RunKind::Changed,
        candidates: vec![njutest_cli::report::CandidateRecord {
            finding: "a".repeat(64),
            mutant: "a".repeat(64),
            kind: "patch".to_owned(),
            path: "tests/closes.rs".to_owned(),
            digest: "b".repeat(64),
            preimage: None,
            stability_runs: 3,
            kill_runs: 2,
            accepted: true,
            why: None,
        }],
        seams: vec![njutest_cli::report::SeamRecord {
            id: "c".repeat(64),
            capability: "api".to_owned(),
            seq: 3,
            asked: "GET /orders".to_owned(),
            answered: Some(200),
            rule: njutest_cli::wire::rule::Rule::StatusServerError,
            decision: njutest_cli::report::SeamDecision::Unnoticed,
        }],
        resources: vec![njutest_cli::report::ResourceRecord {
            capability: "postgres".to_owned(),
            instance: "pg-1".to_owned(),
            environment: vec!["DATABASE_URL".to_owned()],
        }],
        contract: njutest_cli::config::Contract::DeepV1,
        verdict: Verdict::ChangeAssured,
        tool: Tool {
            njutest: "0.1.0".to_owned(),
            rust_mutants: "0.1.0".to_owned(),
        },
        toolchain: Toolchain {
            rustc: "rustc 1.98.0 (0123456789 2026-01-01)".to_owned(),
            cargo: "cargo 1.98.0 (0123456789 2026-01-01)".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
        },
        repository: Repository {
            root_name: "fixture-workspace".to_owned(),
            packages: vec!["app".to_owned(), "core".to_owned()],
            workspace_digest: "a".repeat(64),
            configuration_digest: "b".repeat(64),
            git: Git::Said(njutest_cli::report::Said {
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                branch: "main".to_owned(),
                dirty: true,
                against: Some(njutest_cli::report::Against {
                    merge_base: "fedcba9876543210fedcba9876543210fedcba98".to_owned(),
                    changed_files: vec!["crates/core/src/lib.rs".to_owned()],
                }),
            }),
        },
        scope: Scope {
            requested_packages: vec!["core".to_owned()],
            resolved_packages: vec!["core".to_owned()],
            included: Vec::new(),
            excluded: vec!["**/generated/**".to_owned()],
            configuration: ".njutest.toml".to_owned(),
            shard: None,
        },
        timing: Timing {
            started: "2026-09-05T08:15:00Z".to_owned(),
            finished: "2026-09-05T08:16:12Z".to_owned(),
            duration_ms: 72_000,
        },
        accounting: Accounting {
            targets: TargetAccounting {
                selected: 2,
                passed: 1,
                failed: 0,
                skipped: 1,
                missing: 0,
            },
            mutants: MutantAccounting {
                cataloged: 9,
                rejected: 1,
                executed: 8,
                killed: 7,
                survived: 1,
                runaway: 0,
                waited: 0,
                equivalent: 0,
                unreached: 0,
                accepted: 1,
                reused_killed: 2,
                reused_survived: 0,
                observers: ObserverAccounting {
                    types: 1,
                    tests: 7,
                    steps: 0,
                    proved: 0,
                    unnoticed: 1,
                    unreached: 0,
                    waited: 0,
                    errored: 0,
                },
            },
            soundness: SoundnessAccounting {
                unsafe_items: 3,
                packages_with_unsafe: 1,
                executed: true,
            },
        },
        targets: vec![
            TargetRecord {
                id: "0123456789abcdef".to_owned(),
                name: "core/lib/adds::works".to_owned(),
                package: "core".to_owned(),
                status: TargetStatus::Passed,
                duration_ms: 40,
                message: None,
            },
            TargetRecord {
                id: "fedcba9876543210".to_owned(),
                name: "core/test/it::slow".to_owned(),
                package: "core".to_owned(),
                status: TargetStatus::Skipped,
                duration_ms: 0,
                message: Some("libtest ignored it".to_owned()),
            },
        ],
        mutants: vec![MutantRecord {
            id: "c".repeat(64),
            display_id: "cccccccc".to_owned(),
            path: "crates/core/src/lib.rs".to_owned(),
            position: Position {
                line: 12,
                column: 9,
                character_column: 9,
            },
            rule: "lt-to-le@1".to_owned(),
            item: "demo".to_owned(),
            original: ">".to_owned(),
            replacement: String::new(),
            outcome: njutest_cli::report::Decided::Killed {
                by: "0123456789abcdef".to_owned(),
            },
            reuse: njutest_cli::report::Reuse(njutest_cli::report::Established::ReadBackFrom(
                "20260904T101500Z-123456".to_owned(),
            )),
            blind_in: vec![njutest_cli::report::BlindIn {
                build: "release".to_owned(),
                decision: njutest_cli::report::Blind::Unnoticed,
            }],
            routing: Some(njutest_cli::report::Routing {
                granularity: rust_mutants::session::Granularity::Block,
                reaching: vec!["0123456789abcdef".to_owned()],
                discharged: vec![njutest_cli::report::Discharged {
                    target: "fedcba9876543210".to_owned(),
                    proof: rust_mutants::session::Proof::NeverInfected,
                }],
                fallback: None,
                answered: vec![njutest_cli::report::Answered {
                    target: "0123456789abcdef".to_owned(),
                    outcome: njutest_cli::report::Outcome::parse("killed")
                        .unwrap_or(njutest_cli::report::Outcome::Errored),
                }],
            }),
        }],
        findings: Vec::new(),
        limitations: vec![Limitation::new(
            "doctests-not-routed",
            "doctests run once and are not routed to mutants",
        )],
    }
}

fn schema_path() -> PathBuf {
    njutest_devkit::paths::workspace_root().join("schema/njutest-assurance-report-v1.json")
}

fn schema() -> serde_json::Value {
    let text = std::fs::read_to_string(schema_path()).expect("the published schema");
    serde_json::from_str(&text).expect("the schema is JSON")
}

#[test]
fn the_json_projection_and_schema_carry_an_unmatched_acceptance() {
    let mut report = populated();
    report.verdict = Verdict::Insufficient;
    report.findings = vec![Finding::new(
        FindingKind::UnmatchedAcceptance,
        "ffff",
        "no mutant matches this acceptance",
    )];

    let text = json::document(&report).expect("an audited document");
    let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(document["findings"][0]["kind"], "unmatched-acceptance");
    assert!(
        problems(&document).is_empty(),
        "{}",
        problems(&document).join("\n")
    );
}

fn problems(document: &serde_json::Value) -> Vec<String> {
    let validator = jsonschema::validator_for(&schema()).expect("the schema compiles");
    validator
        .iter_errors(document)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

#[test]
fn the_document_is_indented_and_ends_in_exactly_one_newline() {
    let text = json::document(&populated()).expect("a sound report is written");
    assert!(text.starts_with("{\n  \"schema\": "), "{text}");
    assert!(text.ends_with("}\n"), "ends in a newline");
    assert!(!text.ends_with("}\n\n"), "exactly one");
}

#[test]
fn a_document_reads_back_equal_to_the_report_it_came_from() {
    let report = populated();
    let text = json::document(&report).expect("a sound report is written");
    assert_eq!(json::parse(&text).expect("it reads back"), report);
}

#[test]
fn a_report_that_fails_its_own_audit_is_not_written() {
    let mut report = populated();
    report.accounting.targets.selected = 9;
    let error = json::document(&report).expect_err("the audit refuses it");
    assert_eq!(error.code().code, "NJ6003");
    let rendered = error.to_string();
    assert!(rendered.contains("NJ6003"), "{rendered}");
    assert!(
        rendered.contains("selected 9 targets"),
        "names what is wrong: {rendered}"
    );
}

#[test]
fn a_mutation_that_says_it_was_read_back_from_nobody_is_not_read() {
    let sound = serde_json::to_string(&populated()).expect("a report is a document");
    let read_back = r#""reused":true,"source_run_id":"20260904T101500Z-123456""#;
    for (what, reused, source) in [
        (
            "a disposition read back from an earlier run that names no run",
            "true",
            "null",
        ),
        (
            "a disposition this run established that also names where it came from",
            "false",
            r#""20260904T101500Z-123456""#,
        ),
        (
            "a source run with no name, which is no source at all",
            "true",
            r#""""#,
        ),
    ] {
        let written = sound.replace(
            read_back,
            &format!(r#""reused":{reused},"source_run_id":{source}"#),
        );
        assert_ne!(written, sound, "the document under test was composed");
        let read: Result<Report, serde_json::Error> = serde_json::from_str(&written);
        assert!(
            read.is_err(),
            "{what} is a row where one of the two halves is wrong and a reader cannot \
             tell which. `xtask proofaudit` refuses it too, and keeps doing so on \
             purpose: an audit that stopped checking because the producer's types \
             forbid it would be an audit trusting the producer"
        );
    }
}

#[test]
fn a_document_with_a_field_this_version_does_not_know_is_refused() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    document["accounting"]["targets"]["flaky"] = serde_json::json!(1);
    let error = json::parse(&document.to_string()).expect_err("an unknown field is refused");
    assert_eq!(error.code().code, "NJ6002");
    assert!(error.to_string().contains("flaky"), "{error}");
}

#[test]
fn a_document_missing_a_field_is_refused() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    document
        .as_object_mut()
        .expect("an object")
        .remove("verdict");
    let error = json::parse(&document.to_string()).expect_err("a missing field is refused");
    assert_eq!(error.code().code, "NJ6002");
    assert!(error.to_string().contains("verdict"), "{error}");
}

#[test]
fn the_document_matches_the_recorded_one() {
    let text = json::document(&populated()).expect("a sound report is written");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/report.golden.json");
    njutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded document");
}

#[test]
fn the_published_schema_accepts_a_populated_document() {
    let text = json::document(&populated()).expect("a sound report is written");
    let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert!(problems(&document).is_empty(), "{:?}", problems(&document));
}

#[test]
fn the_published_schema_accepts_the_shard_a_partial_report_records() {
    let mut report = populated();
    report.scope.shard = Some("2/5".to_owned());
    report.verdict = Verdict::Partial;

    let text = json::document(&report).expect("an audited partial report");
    let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(document["scope"]["shard"], "2/5");
    assert!(problems(&document).is_empty(), "{:?}", problems(&document));
}

#[test]
fn the_published_schema_refuses_a_field_the_model_never_writes() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    document["repository"]["remote"] = serde_json::json!("origin");
    assert!(!problems(&document).is_empty(), "every object is closed");
}

#[test]
fn the_published_schema_refuses_a_document_missing_a_field() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    document["timing"]
        .as_object_mut()
        .expect("an object")
        .remove("duration_ms");
    assert!(!problems(&document).is_empty(), "every field is required");
}

/// Closed and complete: the second half of the lock. Without this an object could declare a property the model never writes, and nothing would say so.
#[test]
fn every_object_in_the_schema_is_closed_and_requires_all_it_declares() {
    fn walk(node: &serde_json::Value, at: &str, faults: &mut Vec<String>) {
        let Some(object) = node.as_object() else {
            return;
        };
        if object.get("type").and_then(serde_json::Value::as_str) == Some("object") {
            if object.get("additionalProperties") != Some(&serde_json::json!(false)) {
                faults.push(format!("{at} is not closed"));
            }
            let declared: Vec<&String> = object
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .map(|properties| properties.keys().collect())
                .unwrap_or_default();
            let required: Vec<&str> = object
                .get("required")
                .and_then(serde_json::Value::as_array)
                .map(|items| items.iter().filter_map(serde_json::Value::as_str).collect())
                .unwrap_or_default();
            for name in &declared {
                if !required.contains(&name.as_str()) {
                    faults.push(format!("{at} declares {name} without requiring it"));
                }
            }
        }
        for (key, value) in object {
            walk(value, &format!("{at}/{key}"), faults);
        }
    }

    let mut faults = Vec::new();
    walk(&schema(), "#", &mut faults);
    assert!(faults.is_empty(), "{faults:#?}");
}

/// One mutant object of the populated document, with `changed` folded over it.
fn a_mutant_saying(changed: &serde_json::Value) -> serde_json::Value {
    let text = json::document(&populated()).expect("an audited document");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let held = document
        .get_mut("mutants")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|mutants| mutants.first_mut())
        .expect("a mutant");
    for (key, value) in changed.as_object().expect("an object") {
        held[key] = value.clone();
    }
    document
}

#[test]
fn a_mutation_that_nothing_noticed_and_names_a_noticer_is_not_a_document_this_reads() {
    let document = a_mutant_saying(&serde_json::json!({ "outcome": "survived" }));
    let read = serde_json::from_value::<Report>(document.clone());
    assert!(
        read.is_err(),
        "the record says nothing noticed it and then names the target that did. \
         Carrying that inward and letting a reader meet it is what the pairing being \
         a type prevents, and refusing it at the boundary is where a document written \
         by something else gets caught"
    );
    assert!(
        !problems(&document).is_empty(),
        "and the published schema refuses the same document, because the two say the \
         same thing rather than because somebody checked they agreed"
    );
}

#[test]
fn a_mutation_a_test_noticed_and_names_nobody_is_not_a_document_this_reads() {
    let document = a_mutant_saying(&serde_json::json!({ "killed_by": serde_json::Value::Null }));
    assert!(
        serde_json::from_value::<Report>(document.clone()).is_err(),
        "a kill nobody is named for is the other half of the same defect: a reader \
         is told a test noticed and has nowhere to go"
    );
    assert!(!problems(&document).is_empty());
}

#[test]
fn the_document_a_run_writes_is_the_two_fields_it_always_wrote() {
    let text = json::document(&populated()).expect("an audited document");
    let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let held = &document["mutants"][0];
    assert_eq!(held["outcome"], "killed");
    assert_eq!(
        held["killed_by"], "0123456789abcdef",
        "the pairing is a type inside the program and two fields on the wire, so a \
         document written before it existed still reads: {held}"
    );
}
