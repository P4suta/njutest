// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One document per disposition the published schema declares, because a schema with nine shapes had only ever been shown one.

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
    let schema: Value = serde_json::from_str(&text).expect("the schema is JSON");
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

/// What the schema says about `document`, as sentences.
fn problems(document: &Value) -> Vec<String> {
    validator()
        .iter_errors(document)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

/// The recorded document, with one mutation carrying `outcome` and `killed_by`.
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
    let mut document: Value = serde_json::from_str(&text).expect("the recorded document is JSON");

    let mutation = &mut document["mutants"][0];
    mutation["outcome"] = json!(outcome);
    mutation["killed_by"] = killed_by;
    document
}

/// Every disposition the schema declares, and whether it names a target.
const DISPOSITIONS: [(&str, bool); 9] = [
    ("compile-rejected", false),
    ("killed", true),
    ("runaway", true),
    ("waited", true),
    ("survived", false),
    ("unreached", false),
    ("equivalent", false),
    ("unconfirmed", true),
    ("errored", true),
];

#[test]
fn the_published_schema_accepts_a_document_for_every_disposition_it_declares() {
    for (outcome, names_a_target) in DISPOSITIONS {
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
    for (outcome, names_a_target) in DISPOSITIONS {
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
fn the_two_that_used_to_be_one_are_told_apart_by_their_shape() {
    let runaway = recorded("runaway", json!("0123456789abcdef"));
    let waited = recorded("waited", json!("0123456789abcdef"));
    assert!(problems(&runaway).is_empty() && problems(&waited).is_empty());
    assert_ne!(
        runaway["mutants"][0]["outcome"], waited["mutants"][0]["outcome"],
        "a name that meant two things now means neither, and the two documents \
         differ in the one place a reader looks"
    );
    assert!(
        !problems(&recorded("timed_out", json!("0123456789abcdef"))).is_empty(),
        "and the name that meant both is refused rather than read as whichever \
         of the two a reader guesses, because a fate table that re-reads as one \
         of them is how it starts lying"
    );
}
