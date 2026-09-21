// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an interposer records about one seam, and what a reader gets back from it.

#![expect(
    clippy::indexing_slicing,
    reason = "a test asserts with panics and reads as a table; a record missing where this reads one is the failure it is here to report"
)]

use njutest::wire::{Exchange, Read, SCHEMA, Spoken, read, written};

fn http(seq: u64, path: &str, status: u16) -> Exchange {
    Exchange {
        capability: "api".to_owned(),
        seq,
        during: Some("pkg/test/it orders".to_owned()),
        duration_ms: 12,
        spoken: Spoken::Http {
            method: "GET".to_owned(),
            path: path.to_owned(),
            status,
            request_bytes: 0,
            response_bytes: 84,
            body_bytes: 40,
            status_line: "HTTP/1.1 200 OK".to_owned(),
        },
    }
}

#[expect(
    clippy::panic,
    reason = "a serialization failure is the test failure this helper reports"
)]
fn recorded(exchanges: &[Exchange]) -> String {
    match written(exchanges) {
        Ok(recording) => recording,
        Err(error) => panic!("an Exchange stopped being serializable: {error}"),
    }
}

#[test]
fn what_an_interposer_wrote_is_what_a_reader_gets_back() {
    let held = vec![
        http(0, "/orders", 200),
        Exchange {
            capability: "db".to_owned(),
            seq: 1,
            during: None,
            duration_ms: 3,
            spoken: Spoken::Raw {
                request_bytes: 40,
                response_bytes: 120,
            },
        },
    ];
    let Read { exchanges, unread } = read(&recorded(&held));
    assert_eq!(
        exchanges, held,
        "a catalogue of faults is derived from this recording, so a record that does \
         not come back the way it went in is one that would put a run's questions to \
         a seam nobody spoke to"
    );
    assert_eq!(unread, 0);
}

#[test]
fn a_line_the_reader_cannot_take_is_counted_rather_than_passed_over() {
    let recorded = format!(
        "{}\nnot a record at all\n\n",
        recorded(&[http(0, "/a", 200)]).trim()
    );
    let Read { exchanges, unread } = read(&recorded);
    assert_eq!(exchanges.len(), 1);
    assert_eq!(
        unread, 1,
        "a recording a reader silently shortened is one a derivation would take for \
         a seam that was quieter than it was; the count is what stops an incomplete \
         recording from reading as a complete one"
    );
}

#[test]
fn every_line_names_the_schema_it_answers_to() {
    let recorded = recorded(&[http(0, "/orders", 200)]);
    let first = recorded.lines().next().expect("one line per exchange");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(first).expect("a line is JSON");
    assert_eq!(
        document["schema"], SCHEMA,
        "a stream that does not say what it is is one a later reader has to guess at: \
         {first}"
    );
    assert_eq!(
        document["exchange"]["spoken"]["wire"], "http",
        "and which protocol carried it"
    );
}

#[test]
fn an_exchange_carries_the_test_that_was_running_or_says_it_could_not_tell() {
    let told = http(0, "/orders", 200);
    let untold = Exchange {
        during: None,
        ..told.clone()
    };
    let Read { exchanges, .. } = read(&recorded(&[told, untold]));
    assert_eq!(
        exchanges[0].during.as_deref(),
        Some("pkg/test/it orders"),
        "which test was running is how a fault derived from this exchange is routed \
         back to the tests that could notice it"
    );
    assert_eq!(
        exchanges[1].during, None,
        "and a run that could not tell says so rather than naming the last test it \
         happened to know about"
    );
}

#[test]
fn every_line_a_recording_holds_is_one_the_published_schema_takes() {
    let path = njutest_devkit::paths::workspace_root().join("schema/njutest-wire-v1.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let schema: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");

    let recording = recorded(&[
        http(0, "/orders", 200),
        Exchange {
            capability: "db".to_owned(),
            seq: 1,
            during: None,
            duration_ms: 3,
            spoken: Spoken::Raw {
                request_bytes: 40,
                response_bytes: 120,
            },
        },
    ]);
    for line in recording.lines() {
        let document: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("a line is JSON");
        let problems: Vec<String> = validator
            .iter_errors(&document)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(
            problems.is_empty(),
            "a schema nothing validates a document against is a promise nobody keeps, \
             and this is the document: {line}\n{}",
            problems.join("\n")
        );
    }

    for invalid in [
        serde_json::json!({
            "schema": "njutest-wire-v1",
            "exchange": {
                "capability": "api", "seq": 0, "during": null, "duration_ms": 1,
                "spoken": { "wire": "raw", "request_bytes": 0, "response_bytes": 0 },
                "invented": true
            }
        }),
        serde_json::json!({
            "schema": "njutest-wire-v1",
            "exchange": {
                "capability": "api", "seq": 0, "during": null, "duration_ms": 1,
                "spoken": { "wire": "raw", "request_bytes": 0 }
            }
        }),
        serde_json::json!({
            "schema": "njutest-wire-v1",
            "exchange": {
                "capability": "api", "seq": 0, "during": null, "duration_ms": 1,
                "spoken": { "wire": "future", "request_bytes": 0, "response_bytes": 0 }
            }
        }),
    ] {
        assert!(
            !validator.is_valid(&invalid),
            "the published schema and the Rust types close the same nested objects: {invalid}"
        );
    }
}

#[test]
fn direct_deserialization_rejects_extra_missing_duplicate_and_unknown_input() {
    let valid = r#"{
        "capability":"api",
        "seq":0,
        "during":null,
        "duration_ms":1,
        "spoken":{"wire":"raw","request_bytes":0,"response_bytes":0}
    }"#;
    assert!(
        njutest_devkit::strictjson::decode_str::<Exchange>(valid).is_ok(),
        "the exact object is accepted"
    );

    for invalid in [
        r#"{"capability":"api","seq":0,"during":null,"duration_ms":1,"spoken":{"wire":"raw","request_bytes":0,"response_bytes":0},"invented":true}"#,
        r#"{"capability":"api","seq":0,"during":null,"duration_ms":1,"spoken":{"wire":"raw","request_bytes":0}}"#,
        r#"{"capability":"api","seq":0,"during":null,"duration_ms":1,"spoken":{"wire":"future","request_bytes":0,"response_bytes":0}}"#,
        r#"{"capability":"api","capability":"other","seq":0,"during":null,"duration_ms":1,"spoken":{"wire":"raw","request_bytes":0,"response_bytes":0}}"#,
        r#"{"capability":"api","seq":0,"during":null,"duration_ms":1,"spoken":{"wire":"raw","request_bytes":0,"response_bytes":0,"invented":true}}"#,
    ] {
        assert!(
            njutest_devkit::strictjson::decode_str::<Exchange>(invalid).is_err(),
            "a current owned wire object has exactly one interpretation: {invalid}"
        );
    }
}
