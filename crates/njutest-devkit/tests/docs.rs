// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The documentation-ledger assertion reads exactly one paragraph and fails closed.

use njutest_devkit::docs::{TraceSpecimen, table_count, trace_field_ledger};
use njutest_devkit::result::{ResultState, result_state};
use serde::Serialize;

#[derive(Serialize)]
struct Flat {
    flattened: String,
}

#[derive(Serialize)]
struct Record {
    plain: String,
    optional: Option<u64>,
    #[serde(flatten)]
    flat: Flat,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Payload {
    Nested { nested: Record },
    Direct { first: String, second: u64 },
}

#[derive(Serialize)]
struct Repeated {
    plain: String,
}

#[derive(Serialize)]
struct CollidingRecord {
    plain: String,
    #[serde(flatten)]
    repeated: Repeated,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum CollidingPayload {
    Nested { nested: CollidingRecord },
}

fn nested() -> Payload {
    Payload::Nested {
        nested: Record {
            plain: "one".to_owned(),
            optional: Some(2),
            flat: Flat {
                flattened: "three".to_owned(),
            },
        },
    }
}

fn direct() -> Payload {
    Payload::Direct {
        first: "one".to_owned(),
        second: 2,
    }
}

#[test]
fn only_the_paragraph_immediately_above_the_table_can_answer_for_it() {
    let written = "An earlier paragraph counts three narrowings.\n\n\
                   This table has eleven kinds.\n\n\
                   | `kind` | meaning |\n\
                   | --- | --- |\n";
    assert_eq!(
        table_count(written, "| `kind` |", 11, "kinds").map_err(|error| error.to_string()),
        Ok(())
    );
    assert!(table_count(written, "| `kind` |", 3, "narrowings").is_err());
}

#[test]
fn a_missing_marker_is_an_error() {
    assert!(table_count("eleven kinds", "| `kind` |", 11, "kinds").is_err());
}

#[test]
fn an_unwritten_english_number_is_a_typed_refusal() {
    let result = table_count("twenty-one kinds\n\n| `kind` |", "| `kind` |", 21, "kinds");
    assert_eq!(
        result_state(&result),
        ResultState::Refused,
        "an unsupported count cannot pass"
    );
    if let Err(error) = result {
        assert_eq!(error.to_string(), "no documentation ledger spells 21 yet");
    }
}

#[test]
fn a_trace_field_table_equals_nested_flattened_optional_and_direct_fields() {
    let nested = nested();
    let direct = direct();
    let specimens = [
        TraceSpecimen::new(&nested, Some("nested")),
        TraceSpecimen::new(&direct, None),
    ];
    let page = "| Type | Fields | Records |\n\
                | --- | --- | --- |\n\
                | `nested` | `plain`, `optional`, `flattened` | nested record |\n\
                | `direct` | `first`, `second` | direct record |\n";
    assert_eq!(
        trace_field_ledger(page, "| Type | Fields | Records |", &specimens)
            .map_err(|error| error.to_string()),
        Ok(())
    );
}

#[test]
fn a_trace_field_table_fails_closed_on_missing_extra_and_duplicate_fields() {
    let nested = nested();
    let specimens = [TraceSpecimen::new(&nested, Some("nested"))];
    for (what, fields) in [
        ("missing", "`plain`, `flattened`"),
        ("extra", "`plain`, `optional`, `flattened`, `invented`"),
        ("duplicate", "`plain`, `optional`, `flattened`, `plain`"),
    ] {
        let page = format!(
            "| Type | Fields | Records |\n\
             | --- | --- | --- |\n\
             | `nested` | {fields} | nested record |\n"
        );
        let result = trace_field_ledger(&page, "| Type | Fields | Records |", &specimens);
        assert!(result.is_err(), "{what} fields must be refused: {result:?}");
    }
}

#[test]
fn a_trace_field_table_fails_closed_on_duplicate_missing_and_extra_types() {
    let nested = nested();
    let direct = direct();
    let both = [
        TraceSpecimen::new(&nested, Some("nested")),
        TraceSpecimen::new(&direct, None),
    ];
    let duplicate = "| Type | Fields | Records |\n\
                     | --- | --- | --- |\n\
                     | `nested` | `plain`, `optional`, `flattened` | one |\n\
                     | `nested` | `plain`, `optional`, `flattened` | two |\n";
    assert!(
        trace_field_ledger(duplicate, "| Type | Fields | Records |", &both).is_err(),
        "a repeated type is two sources of truth"
    );

    let missing = "| Type | Fields | Records |\n\
                   | --- | --- | --- |\n\
                   | `nested` | `plain`, `optional`, `flattened` | one |\n";
    assert!(
        trace_field_ledger(missing, "| Type | Fields | Records |", &both).is_err(),
        "a serialized type with no row is refused"
    );

    let nested_only = [TraceSpecimen::new(&nested, Some("nested"))];
    let extra = "| Type | Fields | Records |\n\
                 | --- | --- | --- |\n\
                 | `nested` | `plain`, `optional`, `flattened` | one |\n\
                 | `invented` | `field` | no specimen |\n";
    assert!(
        trace_field_ledger(extra, "| Type | Fields | Records |", &nested_only,).is_err(),
        "a row no payload can serialize is refused"
    );
}

#[test]
fn a_trace_field_table_requires_the_exact_three_column_shape() {
    let nested = nested();
    let specimens = [TraceSpecimen::new(&nested, Some("nested"))];
    for malformed in [
        "| Type | Fields | Records |\n| --- | --- |\n",
        "| Type | Fields | Records |\n| --- |  | --- |\n",
        "| Type | Fields | Records |\n| --- | :--- | --- |\n",
        "| Type | Fields | Records |\n| --- | --- | --- |\n| `nested` | `plain` | prose | extra |\n",
    ] {
        assert!(
            trace_field_ledger(malformed, "| Type | Fields | Records |", &specimens,).is_err(),
            "a malformed table must not become a partial ledger: {malformed:?}"
        );
    }
}

#[test]
fn flattened_serialization_may_not_emit_one_field_twice() {
    let payload = CollidingPayload::Nested {
        nested: CollidingRecord {
            plain: "one".to_owned(),
            repeated: Repeated {
                plain: "two".to_owned(),
            },
        },
    };
    let specimens = [TraceSpecimen::new(&payload, Some("nested"))];
    let page = "| Type | Fields | Records |\n\
                | --- | --- | --- |\n\
                | `nested` | `plain` | nested record |\n";
    assert!(
        trace_field_ledger(page, "| Type | Fields | Records |", &specimens).is_err(),
        "a JSON map would otherwise keep one of the two values and hide the collision"
    );
}
