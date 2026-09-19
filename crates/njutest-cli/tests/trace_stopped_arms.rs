// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One document per way a process stops, against the published trace schema, and the pair that shape replaced.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use serde_json::{Value, json};

fn validator() -> jsonschema::Validator {
    let path = njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json");
    let text = std::fs::read_to_string(&path).expect("the published schema");
    let schema: Value = serde_json::from_str(&text).expect("the schema is JSON");
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

fn event(stopped: Value) -> Value {
    let mut exec = json!({
        "argv": ["cargo", "test"],
        "duration_ms": 12
    });
    if !stopped.is_null() {
        exec["stopped"] = stopped;
    }
    json!({
        "seq": 1,
        "timestamp": "2026-09-19T10:00:00Z",
        "elapsed_ms": 12,
        "type": "exec",
        "exec": exec
    })
}

fn refused(document: &Value) -> Vec<String> {
    validator()
        .iter_errors(document)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect()
}

#[test]
fn every_way_a_process_stops_is_a_shape_the_trace_schema_reads() {
    for shape in [
        json!({ "kind": "ran", "code": 0 }),
        json!({ "kind": "ran", "code": 101 }),
        json!({ "kind": "waited" }),
        json!({ "kind": "runaway" }),
        json!({ "kind": "unstarted" }),
    ] {
        let document = event(shape.clone());
        assert!(
            refused(&document).is_empty(),
            "{shape} is a way a process stops and the schema refuses it: {:?}",
            refused(&document)
        );
    }
}

#[test]
fn a_code_exists_only_where_one_is_the_process_own() {
    for wrong in [
        json!({ "kind": "waited", "code": 101 }),
        json!({ "kind": "runaway", "code": 95 }),
        json!({ "kind": "unstarted", "code": 0 }),
        json!({ "kind": "ran" }),
        json!({ "kind": "timed_out" }),
    ] {
        let document = event(wrong.clone());
        assert!(
            !refused(&document).is_empty(),
            "{wrong} says a process was stopped by something other than itself and \
             also exited a code of its own, which is the pair this replaced"
        );
    }
}

#[test]
fn the_pair_this_replaced_is_no_longer_a_document_this_reads() {
    let mut document = event(Value::Null);
    document["exec"]["exit_code"] = json!(0);
    document["exec"]["timed_out"] = json!(false);
    assert!(
        !refused(&document).is_empty(),
        "an exec record carrying the old two fields and no stopped is not one \
         this release reads"
    );
}
