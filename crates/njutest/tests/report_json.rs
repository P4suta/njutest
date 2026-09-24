// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The canonical projection of a report, and the schema that publishes it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use njutest::report::{
    BuildReport, Finding, FindingKind, Git, LatticedDocument, Limitation, ModelArtifact,
    ModelDecision, ModelIdentity, ModelRecord, MutantAccounting, MutantRecord, ObserverAccounting,
    Position, Report, ReportDocument, Repository, RunKind, Scope, SoundnessAccounting,
    TargetRecord, TargetStatus, Timing, Tool, Toolchain, Verdict, json,
};
use rust_mutants::id::RunId;

#[derive(Debug, thiserror::Error)]
enum FixtureError {
    #[error(transparent)]
    Configured(#[from] njutest::report::across::ConfiguredError),
    #[error(transparent)]
    Completion(#[from] njutest::report::CompletionError),
    #[error("the populated fixture produced a shard")]
    UnexpectedShard,
}

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical run id")
}

fn populated_lattice_from(
    measurements: &njutest::report::across::BuildMeasurements,
) -> Result<LatticedDocument, njutest::report::across::ConfiguredError> {
    njutest::report::across::configured(&run_id("20260905t081500z-abcdef"), measurements)
}

/// A report with something in every field, so the schema is exercised whole.
fn populated_draft(vary: &dyn Fn(&mut BuildReport)) -> BuildReport {
    let mut source = BuildReport::new(
        "source-abcdef",
        RunKind::Changed,
        njutest::config::Contract::DeepV1,
    );
    source.provenance = njutest::report::Provenance {
        identity: "f".repeat(64),
        facts: njutest::report::Established::Here,
    };
    source.candidates = vec![njutest::report::CandidateRecord {
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
    }];
    source.seams = vec![njutest::report::SeamRecord {
        id: "c".repeat(64),
        capability: "api".to_owned(),
        seq: 3,
        asked: "GET /orders".to_owned(),
        answered: Some(200),
        rule: njutest::wire::rule::Rule::StatusServerError,
        decision: njutest::report::SeamDecision::Unnoticed,
    }];
    source.resources = vec![njutest::report::ResourceRecord {
        capability: "postgres".to_owned(),
        instance: "pg-1".to_owned(),
        environment: vec!["DATABASE_URL".to_owned()],
    }];
    source.tool = Tool {
        njutest: "0.1.0".to_owned(),
        rust_mutants: "0.1.0".to_owned(),
    };
    source.toolchain = Toolchain {
        rustc: "rustc 1.98.0 (0123456789 2026-01-01)".to_owned(),
        cargo: "cargo 1.98.0 (0123456789 2026-01-01)".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
    };
    source.repository = Repository {
        root_name: "fixture-workspace".to_owned(),
        packages: vec!["app".to_owned(), "core".to_owned()],
        workspace_digest: "a".repeat(64),
        configuration_digest: "b".repeat(64),
        git: Git::Said(njutest::report::Said {
            commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            branch: "main".to_owned(),
            dirty: true,
            against: Some(njutest::report::Against {
                merge_base: "fedcba9876543210fedcba9876543210fedcba98".to_owned(),
                changed_files: vec!["crates/core/src/lib.rs".to_owned()],
            }),
        }),
    };
    source.scope = Scope {
        requested_packages: vec!["core".to_owned()],
        resolved_packages: vec!["core".to_owned()],
        included: Vec::new(),
        excluded: vec!["**/generated/**".to_owned()],
        configuration: ".njutest.toml".to_owned(),
        configured_builds: vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()],
        shard: None,
    };
    source.timing = Timing {
        started: "2026-09-05T08:15:00Z".to_owned(),
        finished: "2026-09-05T08:16:12Z".to_owned(),
        duration_ms: 72_000,
    };
    source.targets = vec![
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
    ];
    source.mutants = vec![MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "cccccccccccccccccccc".to_owned(),
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
        outcome: njutest::report::Decided::Killed {
            by: "0123456789abcdef".to_owned(),
        },
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::ReadBackFrom(
            "20260904T101500Z-123456".to_owned(),
        )),
        blind_in: Vec::new(),
        routing: Some(njutest::report::Routing {
            granularity: rust_mutants::session::Granularity::Block,
            reaching: vec!["0123456789abcdef".to_owned()],
            discharged: vec![njutest::report::Discharged {
                target: "fedcba9876543210".to_owned(),
                proof: rust_mutants::session::Proof::NeverInfected,
            }],
            fallback: None,
            answered: vec![njutest::report::Answered {
                target: "0123456789abcdef".to_owned(),
                outcome: njutest::report::Outcome::parse("killed")
                    .unwrap_or(njutest::report::Outcome::Errored),
            }],
        }),
    }];
    source.findings = Vec::new();
    source.limitations = vec![Limitation::new(
        rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE,
        "doctests run once and are routed to mutants by the file they are in",
    )];
    source.count_targets().expect("one exact target accounting");
    source.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        killed: 1,
        reused_killed: 1,
        observers: ObserverAccounting {
            tests: 1,
            ..ObserverAccounting::default()
        },
        ..MutantAccounting::default()
    };
    source.accounting.soundness = SoundnessAccounting {
        unsafe_items: 3,
        packages_with_unsafe: 1,
        executed: true,
    };
    njutest::testkit::read_every_named_file(&mut source);
    source.verdict = source.concluded();
    vary(&mut source);
    source
}

fn populated_varying(vary: &dyn Fn(&mut BuildReport)) -> Result<Report, FixtureError> {
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        populated_draft(vary),
    )])
    .map_err(|refused| {
        FixtureError::Configured(njutest::report::across::ConfiguredError::Measurements(
            refused,
        ))
    })?;
    let latticed = populated_lattice_from(&measurements)?;
    let LatticedDocument::Complete(latticed) = latticed else {
        return Err(FixtureError::UnexpectedShard);
    };
    Ok(latticed.complete_without_models()?)
}

/// The populated draft, cut as the second of five shards, holding catalog index one.
fn populated_shard_source() -> BuildReport {
    populated_draft(&|source| {
        source.scope.shard = Some("2/5".to_owned());
        source.mutants[0].catalog_index = njutest::report::CatalogIndex::new(1);
    })
}

fn populated() -> Report {
    populated_varying(&|_| {}).expect("one sound populated fixture")
}

fn schema_path() -> PathBuf {
    njutest_devkit::paths::workspace_root().join("schema/njutest-assurance-report-v1.json")
}

fn schema() -> serde_json::Value {
    let text = std::fs::read_to_string(schema_path()).expect("the published schema");
    njutest_devkit::strictjson::decode_str(&text).expect("the schema is JSON")
}

#[test]
fn a_model_answer_is_nested_beneath_its_identity() {
    let record = serde_json::from_value::<ModelRecord>(serde_json::json!({
        "mutant": "a".repeat(64),
        "answer": { "decision": "ineligible", "reason": "effect" }
    }))
    .expect("checked model record");
    let document = serde_json::to_value(&record).expect("model record");
    assert_eq!(
        document,
        serde_json::json!({
            "mutant": "a".repeat(64),
            "answer": { "decision": "ineligible", "reason": "effect" }
        })
    );
    let flattened = serde_json::json!({
        "mutant": "a".repeat(64),
        "decision": "ineligible",
        "reason": "effect"
    });
    let Err(refused) = serde_json::from_value::<ModelRecord>(flattened) else {
        panic!("v1 never merges answer fields into the record identity namespace")
    };
    drop(refused);
}

fn model_identity_json() -> serde_json::Value {
    let path = "src/lib.rs";
    let rule = "replace-binop";
    let source = "b".repeat(64);
    let original = b"+";
    let replacement = b"-";
    let start = 10;
    let end = 11;
    let mutant = rust_mutants::id::Identity {
        path: path.to_owned(),
        rule_name: rule.to_owned(),
        rule_version: 1,
        span: rust_mutants::span::Span::new(start, end).expect("valid fixture span"),
        source_digest: source.clone(),
        original_digest: rust_mutants::id::digest(original),
        replacement_digest: rust_mutants::id::digest(replacement),
    }
    .id()
    .expect("valid fixture mutation identity");
    let rendered = "c".repeat(64);
    serde_json::json!({
        "harness": format!("__njutest_model_{mutant}"),
        "assertion": format!("njutest-model-v1:{mutant}"),
        "unwind": 1,
        "timeout_ms": 1,
        "source_sha256": source,
        "rendered_sha256": rendered,
        "crate_input": {
            "package": "njutest-verified-model",
            "edition": "2024",
            "source": "src/lib.rs",
            "offline": true,
            "dependency_resolution": "empty-lock-offline-v1",
            "environment": "minimal-v1",
            "sha256": model_crate_sha256(&"c".repeat(64))
        },
        "mutant": mutant,
        "path": path,
        "rule": rule,
        "rule_version": 1,
        "start_byte": start,
        "end_byte": end,
        "original_hex": hex::encode(original),
        "replacement_hex": hex::encode(replacement)
    })
}

fn model_crate_sha256(rendered_sha256: &str) -> String {
    let manifest = "[package]\nname = \"njutest-verified-model\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\nautobins = false\nautoexamples = false\nautotests = false\nautobenches = false\n\n[lib]\npath = \"src/lib.rs\"\n\n[workspace]\n";
    let lock = "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"njutest-verified-model\"\nversion = \"0.0.0\"\n";
    let ledger = format!(
        "njutest-model-crate-v1\nenvironment:minimal-v1\ndependency-resolution:empty-lock-offline-v1\nCargo.toml:{}\nCargo.lock:{}\nsrc/lib.rs:{rendered_sha256}\n",
        rust_mutants::id::digest(manifest.as_bytes()),
        rust_mutants::id::digest(lock.as_bytes()),
    );
    rust_mutants::id::digest(ledger.as_bytes())
}

fn model_artifact_json(path: &str, bytes: u64, sha256: &str) -> serde_json::Value {
    serde_json::json!({ "path": path, "bytes": bytes, "sha256": sha256 })
}

fn model_verifier_json() -> serde_json::Value {
    serde_json::json!({
        "tool": "0.68.0",
        "backend": {
            "export_version": "1.0",
            "build_mode": "release",
            "target": "x86_64-unknown-linux-gnu",
            "rustc": "rustc 1.100.0-nightly (8925ea358 2026-08-20)",
            "cbmc": "6.11.0 (cbmc-6.11.0)",
            "goto_cc": "clang version 21.0.0 (goto-cc 6.11.0 (cbmc-6.11.0))",
            "goto_instrument": "6.11.0 (cbmc-6.11.0)",
            "solver": "cadical"
        }
    })
}

#[test]
fn model_identity_bounds_and_digests_are_checked_by_deserialization_types() {
    let valid = model_identity_json();
    serde_json::from_value::<ModelIdentity>(valid.clone()).expect("typed model identity");

    for (field, value) in [
        ("unwind", serde_json::json!(0)),
        ("timeout_ms", serde_json::json!(0)),
        ("rule_version", serde_json::json!(0)),
        ("source_sha256", serde_json::json!("not-a-digest")),
        ("rendered_sha256", serde_json::json!("A".repeat(64))),
        ("mutant", serde_json::json!("0".repeat(63))),
        ("path", serde_json::json!("../src/lib.rs")),
        ("path", serde_json::json!("C:src/lib.rs")),
        ("path", serde_json::json!("src//lib.rs")),
        ("path", serde_json::json!("./src/lib.rs")),
        ("rule", serde_json::json!("bad rule")),
        ("original_hex", serde_json::json!("2B")),
        ("replacement_hex", serde_json::json!("2b")),
        ("end_byte", serde_json::json!(12)),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let Err(refused) = serde_json::from_value::<ModelIdentity>(invalid) else {
            panic!("{field} cannot inhabit a model identity")
        };
        drop(refused);
    }

    for (field, value) in [
        ("package", serde_json::json!("subject-package")),
        ("edition", serde_json::json!("2021")),
        ("source", serde_json::json!("src/main.rs")),
        ("offline", serde_json::json!(false)),
        ("dependency_resolution", serde_json::json!("ambient")),
        ("environment", serde_json::json!("ambient")),
        ("sha256", serde_json::json!("E".repeat(64))),
        ("sha256", serde_json::json!("f".repeat(64))),
    ] {
        let mut invalid = valid.clone();
        invalid["crate_input"][field] = value;
        let Err(refused) = serde_json::from_value::<ModelIdentity>(invalid) else {
            panic!("crate_input.{field} cannot change the proof crate contract")
        };
        drop(refused);
    }
}

#[test]
fn model_artifacts_are_nonempty_canonical_relative_files() {
    let valid = model_artifact_json(
        &format!("model/{}.json", "a".repeat(64)),
        2,
        &"d".repeat(64),
    );
    serde_json::from_value::<ModelArtifact>(valid.clone()).expect("typed artifact");
    for (field, value) in [
        ("path", serde_json::json!("/tmp/result.json")),
        ("path", serde_json::json!("model/../result.json")),
        ("path", serde_json::json!("model\\result.json")),
        ("bytes", serde_json::json!(0)),
        ("sha256", serde_json::json!("D".repeat(64))),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let Err(refused) = serde_json::from_value::<ModelArtifact>(invalid) else {
            panic!("{field} cannot inhabit a complete model artifact")
        };
        drop(refused);
    }
}

#[test]
fn affirmative_model_evidence_has_a_pinned_backend_and_fixed_exit() {
    let identity = model_identity_json();
    let mutant = identity["mutant"].clone();
    let mutant_text = mutant.as_str().expect("fixture mutation identity");
    let evidence = serde_json::json!({
        "verifier": model_verifier_json(),
        "identity": identity,
        "artifact": model_artifact_json(&format!("model/{mutant_text}.json"), 2, &"d".repeat(64)),
        "source": model_artifact_json(&format!("model/{mutant_text}.rs"), 2, &"c".repeat(64)),
        "process": { "kind": "exited", "code": 0 }
    });
    let proved = serde_json::json!({ "decision": "proved", "evidence": evidence });
    serde_json::from_value::<ModelDecision>(proved.clone()).expect("typed proof evidence");
    serde_json::from_value::<ModelRecord>(serde_json::json!({
        "mutant": mutant,
        "answer": proved
    }))
    .expect("record identity is coupled to proof identity");

    let mut wrong_exit = proved.clone();
    wrong_exit["evidence"]["process"]["code"] = serde_json::json!(1);
    let Err(refused) = serde_json::from_value::<ModelDecision>(wrong_exit) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);

    let mut wrong_backend = proved.clone();
    wrong_backend["evidence"]["verifier"]["backend"]["solver"] = serde_json::json!("untrusted");
    let Err(refused) = serde_json::from_value::<ModelDecision>(wrong_backend) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);

    let mut wrong_source = proved.clone();
    wrong_source["evidence"]["source"]["sha256"] = serde_json::json!("e".repeat(64));
    let Err(refused) = serde_json::from_value::<ModelDecision>(wrong_source) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);

    let mut wrong_record = serde_json::json!({
        "mutant": "f".repeat(64),
        "answer": proved
    });
    let Err(refused) = serde_json::from_value::<ModelRecord>(wrong_record.take()) else {
        panic!("the forged record must be refused")
    };
    drop(refused);
}

#[test]
fn undecided_model_attempts_are_one_closed_reason_evidence_pair() {
    let identity = model_identity_json();
    let mutant = identity["mutant"]
        .as_str()
        .expect("fixture mutation identity")
        .to_owned();
    let valid = serde_json::json!({
        "decision": "undecided",
        "attempt": {
            "reason": { "kind": "tool", "detail": "unavailable" },
            "evidence": {
                "verifier": null,
                "identity": identity,
                "artifact": null,
                "source": model_artifact_json(&format!("model/{mutant}.rs"), 2, &"c".repeat(64)),
                "process": { "kind": "not-run" },
                "raw_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            }
        }
    });
    serde_json::from_value::<ModelDecision>(valid.clone()).expect("typed undecided attempt");

    for nullable in ["verifier", "artifact"] {
        let mut missing = valid.clone();
        missing["attempt"]["evidence"]
            .as_object_mut()
            .expect("the evidence object")
            .remove(nullable);
        let Err(refused) = serde_json::from_value::<ModelDecision>(missing) else {
            panic!("{nullable} must be explicitly null rather than absent")
        };
        drop(refused);
    }

    let mut mismatched_process = valid.clone();
    mismatched_process["attempt"]["reason"] = serde_json::json!({ "kind": "cutoff" });
    let Err(refused) = serde_json::from_value::<ModelDecision>(mismatched_process) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);

    let mut phantom_raw = valid.clone();
    phantom_raw["attempt"]["evidence"]["raw_sha256"] = serde_json::json!("d".repeat(64));
    let Err(refused) = serde_json::from_value::<ModelDecision>(phantom_raw) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);

    let flat = serde_json::json!({
        "decision": "undecided",
        "reason": valid["attempt"]["reason"].clone(),
        "evidence": valid["attempt"]["evidence"].clone()
    });
    let Err(refused) = serde_json::from_value::<ModelDecision>(flat) else {
        panic!("the malformed evidence must be refused")
    };
    drop(refused);
}

#[test]
fn the_json_projection_and_schema_carry_an_unmatched_acceptance() {
    let report = populated_varying(&|source| {
        source.findings.push(Finding::new(
            FindingKind::UnmatchedAcceptance,
            "ffff",
            "no mutant matches this acceptance",
        ));
    })
    .expect("one sound fixture carrying an unmatched acceptance");

    let text = json::document(&report).expect("an audited document");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    assert_eq!(
        document["report"]["global_findings"][0]["kind"],
        "unmatched-acceptance"
    );
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
    assert!(
        text.starts_with(
            "{\n  \"document_type\": \"complete\",
  \"report\": {"
        ),
        "{text}"
    );
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
fn a_report_whose_rows_and_counters_disagree_is_not_completed() {
    let refused = populated_varying(&|source| {
        source.accounting.targets.selected = 9;
    })
    .expect_err("the checked completion refuses it");
    let rendered = refused.to_string();
    assert!(
        rendered.contains("target accounting"),
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
        let read: Result<Report, serde_json::Error> =
            njutest_devkit::strictjson::decode_str(&written);
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
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    document["report"]["builds"][0]["parts"][0]["accounting"]["targets"]["flaky"] =
        serde_json::json!(1);
    let error = json::parse(&document.to_string()).expect_err("an unknown field is refused");
    assert_eq!(error.code().code, "NJ6002");
    assert!(error.to_string().contains("flaky"), "{error}");
}

#[test]
fn a_document_missing_a_field_is_refused() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    document["report"]
        .as_object_mut()
        .expect("an object")
        .remove("scope");
    let error = json::parse(&document.to_string()).expect_err("a missing field is refused");
    assert_eq!(error.code().code, "NJ6002");
    assert!(error.to_string().contains("scope"), "{error}");
}

#[test]
fn every_nullable_report_field_must_be_present_even_when_null() {
    fn remove(document: &mut serde_json::Value, pointer: &str) {
        let (parent, field) = pointer.rsplit_once('/').expect("a JSON pointer to a field");
        let removed = document
            .pointer_mut(parent)
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|object| object.remove(field));
        assert!(removed.is_some(), "the fixture carries {pointer}");
    }

    let text = json::document(&populated()).expect("a sound report is written");
    let exact: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the exact report");
    for pointer in [
        "/report/provenance/source_run_id",
        "/report/repository/git/merge_base",
        "/report/scope/shard",
        "/report/builds/0/configuration/profile",
        "/report/builds/0/configuration/target",
        "/report/builds/0/configuration/jobs",
        "/report/builds/0/parts/0/candidates/0/preimage",
        "/report/builds/0/parts/0/candidates/0/why",
        "/report/builds/0/parts/0/seams/0/answered",
        "/report/builds/0/parts/0/targets/0/message",
        "/report/builds/0/parts/0/mutants/0/decision/killed_by",
        "/report/builds/0/parts/0/mutants/0/decision/step_boundary",
        "/report/builds/0/parts/0/mutants/0/routing",
        "/report/builds/0/parts/0/mutants/0/routing/fallback",
        "/report/builds/0/parts/0/mutants/0/reuse/source_run_id",
    ] {
        let mut missing = exact.clone();
        remove(&mut missing, pointer);
        let error = json::parse(&missing.to_string()).expect_err("absence is not null");
        assert_eq!(error.code().code, "NJ6002", "{pointer}: {error}");
    }

    let mut with_finding = exact;
    with_finding["report"]["builds"][0]["parts"][0]["findings"] =
        serde_json::json!([serde_json::to_value(Finding::new(
            FindingKind::WaitedMutant,
            "subject",
            "detail"
        ))
        .expect("a finding serializes")]);
    for pointer in [
        "/report/builds/0/parts/0/findings/0/path",
        "/report/builds/0/parts/0/findings/0/position",
    ] {
        let mut missing = with_finding.clone();
        remove(&mut missing, pointer);
        let error = json::parse(&missing.to_string()).expect_err("absence is not null");
        assert_eq!(error.code().code, "NJ6002", "{pointer}: {error}");
    }
}

#[test]
fn duplicate_keys_are_refused_before_the_report_is_projected() {
    let text = json::document(&populated()).expect("a sound report is written");
    let duplicate_root = text.replacen("{\n", "{\n  \"document_type\": \"complete\",\n", 1);
    let duplicate_nested = text.replacen(
        "\"selected\": 2,",
        "\"selected\": 2,\n      \"selected\": 2,",
        1,
    );
    for malformed in [duplicate_root, duplicate_nested] {
        let error = json::parse(&malformed).expect_err("duplicate names are ambiguous");
        assert_eq!(error.code().code, "NJ6002");
        assert!(
            error.to_string().contains("duplicate JSON object key"),
            "{error}"
        );
    }
}

#[test]
fn the_reader_rederives_cross_field_facts_instead_of_trusting_them() {
    let text = json::document(&populated()).expect("a sound report is written");
    let original: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    let mut contradictions = Vec::new();

    let mut accounting = original.clone();
    accounting["report"]["builds"][0]["parts"][0]["accounting"]["targets"]["selected"] =
        serde_json::json!(9);
    contradictions.push(("accounting", accounting));

    let mut ledger = original.clone();
    ledger["report"]["builds"][0]["name"] = serde_json::json!("release");
    contradictions.push(("build ledger", ledger));

    let mut provenance = original.clone();
    provenance["report"]["provenance"]["cached"] = serde_json::json!(true);
    provenance["report"]["provenance"]["source_run_id"] = serde_json::Value::Null;
    contradictions.push(("provenance", provenance));

    let mut impossible_acceptance = original.clone();
    impossible_acceptance["report"]["builds"][0]["parts"][0]["mutants"][0]["accepted"] =
        serde_json::json!(true);
    impossible_acceptance["report"]["builds"][0]["parts"][0]["accounting"]["mutants"]["accepted"] =
        serde_json::json!(1);
    contradictions.push(("acceptance attached to a kill", impossible_acceptance));

    let mut model = original;
    model["report"]["contract"] = serde_json::json!("verified-v1");
    contradictions.push(("model correlation", model));

    for (name, document) in contradictions {
        let error =
            json::parse(&document.to_string()).expect_err("a contradiction is never a report");
        assert_eq!(
            error.code().code,
            "NJ6002",
            "{name} is refused inside the checked reader, before any document is trusted: {error}"
        );
    }
}

#[test]
fn target_rows_rederive_every_status_and_refuse_duplicate_identities() {
    let wrong_status = populated_varying(&|source| {
        source.targets[0].status = TargetStatus::Failed;
    })
    .expect_err("changing a row without its counters is detected from the rows");
    assert!(
        wrong_status.to_string().contains("target accounting"),
        "counters are derived, never trusted: {wrong_status}"
    );

    let duplicate = populated_varying(&|source| {
        source.targets[1].id = source.targets[0].id.clone();
    })
    .expect_err("two measurements cannot claim one target identity");
    assert!(
        duplicate.to_string().contains("duplicate"),
        "a repeated identity is refused with the rows: {duplicate}"
    );

    let reordered = populated_varying(&|source| {
        source.targets.reverse();
    })
    .expect_err("a durable report has one canonical target order");
    assert!(
        reordered.to_string().contains("canonical order"),
        "order is checked where the rows enter evidence: {reordered}"
    );
}

#[test]
fn mutant_rows_rederive_every_counter_and_refuse_duplicate_identities() {
    let wrong_count = populated_varying(&|source| {
        source.accounting.mutants.killed = 0;
    })
    .expect_err("a balanced but invented counter cannot replace the row-derived fact");
    assert!(
        wrong_count.to_string().contains("mutation accounting"),
        "counters are derived, never trusted: {wrong_count}"
    );

    let duplicate = populated_varying(&|source| {
        source.accounting.mutants = MutantAccounting {
            cataloged: 2,
            executed: 2,
            killed: 2,
            reused_killed: 2,
            observers: ObserverAccounting {
                tests: 2,
                ..ObserverAccounting::default()
            },
            ..MutantAccounting::default()
        };
        let mut repeated = source.mutants[0].clone();
        repeated.catalog_index = njutest::report::CatalogIndex::new(1);
        source.mutants.push(repeated);
    })
    .expect_err("one catalog identity cannot occur twice");
    assert!(
        duplicate.to_string().contains("occurs more than once"),
        "the completed part set names the repeated identity: {duplicate}"
    );
}

/// The row's route answered as a survivor's is: every target it asked ran it and did not notice.
fn answered_as_a_survivor(row: &mut MutantRecord) {
    for one in row
        .routing
        .iter_mut()
        .flat_map(|routing| &mut routing.answered)
    {
        one.outcome = njutest::report::Outcome::Survived;
    }
}

#[test]
fn every_survivor_and_affirmative_model_outcome_has_exactly_one_model_record() {
    for outcome in [
        njutest::report::Decided::Survived,
        njutest::report::Decided::ModelNoticed,
        njutest::report::Decided::ModelProved,
    ] {
        let refused = populated_varying(&|source| {
            source.contract = njutest::config::Contract::VerifiedV1;
            source.mutants[0].outcome = outcome.clone();
            source.mutants[0].reuse = njutest::report::Reuse(njutest::report::Established::Here);
            answered_as_a_survivor(&mut source.mutants[0]);
            source.accounting.mutants = match &outcome {
                njutest::report::Decided::Survived => MutantAccounting {
                    cataloged: 1,
                    executed: 1,
                    survived: 1,
                    observers: ObserverAccounting {
                        unnoticed: 1,
                        ..ObserverAccounting::default()
                    },
                    ..MutantAccounting::default()
                },
                njutest::report::Decided::ModelNoticed => MutantAccounting {
                    cataloged: 1,
                    executed: 1,
                    model_noticed: 1,
                    observers: ObserverAccounting {
                        model_noticed: 1,
                        ..ObserverAccounting::default()
                    },
                    ..MutantAccounting::default()
                },
                njutest::report::Decided::ModelProved => MutantAccounting {
                    cataloged: 1,
                    executed: 1,
                    model_proved: 1,
                    observers: ObserverAccounting {
                        model_proved: 1,
                        ..ObserverAccounting::default()
                    },
                    ..MutantAccounting::default()
                },
                njutest::report::Decided::CompileRejected
                | njutest::report::Decided::Killed { .. }
                | njutest::report::Decided::StepLimitReached { .. }
                | njutest::report::Decided::Waited { .. }
                | njutest::report::Decided::Unreached
                | njutest::report::Decided::Equivalent
                | njutest::report::Decided::Unconfirmed { .. }
                | njutest::report::Decided::Errored { .. } => {
                    panic!("the model phase refuses a {outcome:?} row")
                }
            };
            if outcome == njutest::report::Decided::Survived {
                source.findings = vec![Finding::new(
                    FindingKind::SurvivingMutant,
                    &source.mutants[0].display_id.clone(),
                    "no test noticed it",
                )];
            }
        })
        .expect_err("verified-v1 cannot complete without its model batch");
        assert!(
            match outcome {
                njutest::report::Decided::Survived => matches!(
                    refused,
                    FixtureError::Completion(njutest::report::CompletionError::ModelRequired)
                ),
                _ => matches!(
                    refused,
                    FixtureError::Configured(
                        njutest::report::across::ConfiguredError::ModelPipelineRequired
                    )
                ),
            },
            "the row cannot stand in for independently retained model evidence: {refused:?}"
        );
    }
}

#[test]
fn contracts_without_model_checking_refuse_model_outcomes() {
    for contract in [
        njutest::config::Contract::StandardV1,
        njutest::config::Contract::DeepV1,
    ] {
        let refused = populated_varying(&|source| {
            source.contract = contract;
            source.mutants[0].outcome = njutest::report::Decided::ModelNoticed;
        })
        .expect_err("a model decision cannot enter evidence before the final lattice");
        assert!(
            matches!(
                refused,
                FixtureError::Configured(
                    njutest::report::across::ConfiguredError::ModelPipelineRequired
                )
            ),
            "{contract:?} must not admit model-only decisions: {refused}"
        );
    }
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
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    assert!(problems(&document).is_empty(), "{:?}", problems(&document));
}

#[test]
fn the_shard_a_partial_report_records_carries_its_part_and_reads_back() {
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        populated_shard_source(),
    )])
    .expect("one checked build measurement");
    let latticed = populated_lattice_from(&measurements).expect("one checked shard lattice");
    let LatticedDocument::Shard(shard) = latticed else {
        panic!("the sharded fixture cannot be a whole report");
    };
    assert_eq!(shard.verdict(), Verdict::Partial);
    let text =
        json::document_any(&ReportDocument::Shard(shard)).expect("an audited shard document");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    assert_eq!(document["report"]["shard"]["index"], 2);
    assert_eq!(document["report"]["shard"]["of"], 5);
    let read = json::parse_any(&text).expect("the shard reads back");
    assert!(matches!(read, ReportDocument::Shard(_)));
    assert_eq!(read.verdict(), Verdict::Partial);
}

#[test]
fn the_published_schema_refuses_a_field_the_model_never_writes() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    document["report"]["repository"]["remote"] = serde_json::json!("origin");
    assert!(!problems(&document).is_empty(), "every object is closed");
}

#[test]
fn the_published_schema_refuses_acceptance_on_an_already_answered_mutation() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    document["report"]["builds"][0]["parts"][0]["mutants"][0]["accepted"] = serde_json::json!(true);
    document["report"]["builds"][0]["parts"][0]["accounting"]["mutants"]["accepted"] =
        serde_json::json!(1);
    assert!(
        !problems(&document).is_empty(),
        "the v1 schema admits acceptance only on a reviewable gap"
    );
}

#[test]
fn the_published_schema_refuses_a_document_missing_a_field() {
    let text = json::document(&populated()).expect("a sound report is written");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    document["report"]["builds"][0]["parts"][0]["timing"]
        .as_object_mut()
        .expect("an object")
        .remove("duration_ms");
    assert!(!problems(&document).is_empty(), "every field is required");
}

/// Closed and complete: the second half of the lock.
/// Without this an object could declare a property the model never writes, and nothing would say so.
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

/// One mutant decision of the populated document, with `changed` folded over it.
fn a_mutant_saying(changed: &serde_json::Value) -> serde_json::Value {
    let text = json::document(&populated()).expect("an audited document");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    let held = document
        .pointer_mut("/report/builds/0/parts/0/mutants")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|mutants| mutants.first_mut())
        .expect("a mutant");
    let decision = held
        .get_mut("decision")
        .and_then(serde_json::Value::as_object_mut)
        .expect("a nested decision");
    for (key, value) in changed.as_object().expect("an object") {
        decision.insert(key.clone(), value.clone());
    }
    document
}

#[test]
fn a_mutation_that_nothing_noticed_and_names_a_noticer_is_not_a_document_this_reads() {
    let document = a_mutant_saying(&serde_json::json!({ "outcome": "survived" }));
    let read = serde_json::from_value::<ReportDocument>(document.clone());
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
        serde_json::from_value::<ReportDocument>(document.clone()).is_err(),
        "a kill nobody is named for is the other half of the same defect: a reader \
         is told a test noticed and has nowhere to go"
    );
    assert!(!problems(&document).is_empty());
}

#[test]
fn the_document_a_run_writes_is_a_closed_nested_decision() {
    let text = json::document(&populated()).expect("an audited document");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    let held = &document["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"];
    assert_eq!(held["outcome"], "killed");
    assert_eq!(
        held["killed_by"], "0123456789abcdef",
        "the pairing is one closed object both inside the program and on the v1 wire: {held}"
    );
}
