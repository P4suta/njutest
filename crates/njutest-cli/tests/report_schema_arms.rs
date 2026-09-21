// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One document per disposition the published schema declares, because a schema with many shapes had only ever been shown one.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use serde_json::{Value, json};

/// The published schema, compiled.
fn validator() -> jsonschema::Validator {
    let path =
        njutest_devkit::paths::workspace_root().join("schema/njutest-assurance-report-v1.json");
    let text = std::fs::read_to_string(&path).expect("the published schema");
    let schema: Value = njutest_devkit::strictjson::decode_str(&text).expect("the schema is JSON");
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

/// What the schema says about `document`, as sentences.
fn problems(document: &Value) -> Vec<String> {
    validator()
        .iter_errors(document)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

/// The recorded document, with one mutation carrying a closed decision object.
///
/// Built from the golden rather than from the model on purpose: the model
/// writes whichever disposition the run it is given reaches, so a suite that
/// only ever builds one report only ever shows the schema one of its shapes.
/// A document is bytes, and bytes are what the published contract is about.
///
/// Nothing about the accounting is patched. It was, while the model still
/// wrote a single `timed_out` and the schema already said two; the day the
/// golden caught up, the patching started writing nulls into the two columns
/// it had been adding, and these tests failed for the scaffolding rather
/// than for the shape.
fn recorded(outcome: &str, killed_by: Value) -> Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/report.golden.json");
    let text = std::fs::read_to_string(&path).expect("the recorded document");
    let mut document: Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the recorded document is JSON");

    let decision = &mut document["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"];
    decision["outcome"] = json!(outcome);
    decision["killed_by"] = killed_by;
    if outcome == "step-limit-reached" {
        decision["step_boundary"] = json!({ "limit": 10, "observed": 11 });
    } else {
        decision["step_boundary"] = Value::Null;
    }
    document
}

/// Every disposition a report can carry, and whether it names a target.
///
/// Derived, because the list this replaced held eleven string literals beside an eleven-variant set and a twelfth would have been in neither.
fn dispositions() -> Vec<(&'static str, bool)> {
    njutest_cli::report::Decided::every_against("0123456789abcdef")
        .iter()
        .map(|decided| (decided.outcome().name(), decided.decided_by().is_some()))
        .collect()
}

#[test]
fn the_published_schema_accepts_a_document_for_every_disposition_it_declares() {
    for (outcome, names_a_target) in dispositions() {
        let named = if names_a_target {
            json!("0123456789abcdef")
        } else {
            Value::Null
        };
        let document = recorded(outcome, named);
        assert!(
            problems(&document).is_empty(),
            "the schema publishes a shape for {outcome} and refuses a document that \
             is in it: {:?}",
            problems(&document)
        );
    }
}

#[test]
fn a_disposition_that_names_a_target_is_not_one_that_names_nobody() {
    for (outcome, names_a_target) in dispositions() {
        let wrong = if names_a_target {
            Value::Null
        } else {
            json!("0123456789abcdef")
        };
        let document = recorded(outcome, wrong);
        assert!(
            !problems(&document).is_empty(),
            "outcome and killed_by are one thing in the model, so a document that \
             pairs {outcome} with the other answer is not one this release reads. \
             A run that established nothing about a mutation and then named the \
             target it established nothing about would read as a kill"
        );
    }
}

#[test]
fn a_verified_step_boundary_is_distinct_from_waiting_and_from_retired_spellings() {
    let limited = recorded("step-limit-reached", json!("0123456789abcdef"));
    let waited = recorded("waited", json!("0123456789abcdef"));
    assert!(problems(&limited).is_empty() && problems(&waited).is_empty());
    assert_ne!(
        limited["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"]["outcome"],
        waited["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"]["outcome"],
        "a name that meant two things now means neither, and the two documents \
         differ in the one place a reader looks"
    );
    for retired in ["runaway", "timed_out"] {
        assert!(
            !problems(&recorded(retired, json!("0123456789abcdef"))).is_empty(),
            "the retired {retired} spelling is refused rather than reinterpreted"
        );
    }
}

#[test]
fn only_a_step_limit_record_carries_one_verified_boundary() {
    let mut missing = recorded("step-limit-reached", json!("0123456789abcdef"));
    missing["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"]
        .as_object_mut()
        .expect("a typed decision")
        .remove("step_boundary");
    assert!(
        !problems(&missing).is_empty(),
        "a bare outcome name cannot stand in for the nonce-verified boundary"
    );

    let mut extraneous = recorded("waited", json!("0123456789abcdef"));
    extraneous["report"]["builds"][0]["parts"][0]["mutants"][0]["decision"]["step_boundary"] =
        json!({ "limit": 10, "observed": 11 });
    assert!(
        !problems(&extraneous).is_empty(),
        "the boundary belongs to the typed step-limit arm and no other outcome"
    );

    let mut mismatched = recorded("step-limit-reached", json!("0123456789abcdef"));
    mismatched["report"]["builds"][0]["parts"][0]["mutants"][0]["step_boundary"]["observed"] =
        json!(12);
    assert!(
        serde_json::from_value::<njutest_cli::report::Report>(mismatched).is_err(),
        "the Rust wire type proves observed is exactly limit + 1 even though JSON Schema \
         cannot express cross-field arithmetic"
    );
}
