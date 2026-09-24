// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Completing a specimen the sentinels lay with what its test leaves out, so it is on its schema; never reached from a reader, which a gate holds.

use serde_json::Value;

use crate::schemas::Producer;

/// The neutral value of every required field a specimen may leave out, by producer, event type and record: the value a run writes when the field says nothing.
///
/// Identity fields, an index or a target, are never here: a specimen that leaves one out is off its schema, and says so.
const NEUTRAL: [Neutral; 5] = [
    Neutral {
        producer: Producer::Runner,
        event: "route",
        record: "route",
        fields: &[
            ("fallback", NeutralValue::Null),
            ("reaching", NeutralValue::Empty),
            ("tests", NeutralValue::Empty),
            ("discharged", NeutralValue::Empty),
            ("considered", NeutralValue::Empty),
            ("reused", NeutralValue::Null),
            ("refused", NeutralValue::Null),
        ],
    },
    Neutral {
        producer: Producer::Runner,
        event: "mutant-exec",
        record: "mutant",
        fields: &[
            ("args", NeutralValue::Empty),
            ("step_boundary", NeutralValue::Null),
            ("duration_ms", NeutralValue::Zero),
            ("alone", NeutralValue::False),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "route",
        record: "route",
        fields: &[
            ("fallback", NeutralValue::Null),
            ("reaching", NeutralValue::Empty),
            ("discharged", NeutralValue::Empty),
            ("considered", NeutralValue::Empty),
            ("executed", NeutralValue::Empty),
            ("reused", NeutralValue::Null),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "mutant-exec",
        record: "mutant",
        fields: &[
            ("alone", NeutralValue::False),
            ("duration_ms", NeutralValue::Zero),
            ("exit_code", NeutralValue::Zero),
            ("failed_tests", NeutralValue::Empty),
            ("signal", NeutralValue::Null),
            ("step_notice", NeutralValue::Null),
            ("tests_run", NeutralValue::Null),
            ("timeout_ms", NeutralValue::Zero),
            ("timeout_source", NeutralValue::Configured),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "verify",
        record: "verify",
        fields: &[
            ("duration_ms", NeutralValue::Zero),
            ("remembered", NeutralValue::False),
            ("retried", NeutralValue::False),
            ("tests_run", NeutralValue::Null),
        ],
    },
];

/// The neutral fields of one record of one event type of one producer.
#[derive(Debug, Clone, Copy)]
struct Neutral {
    producer: Producer,
    event: &'static str,
    record: &'static str,
    fields: &'static [(&'static str, NeutralValue)],
}

/// A value that says nothing.
#[derive(Debug, Clone, Copy)]
enum NeutralValue {
    Null,
    Empty,
    Zero,
    False,
    Configured,
}

impl NeutralValue {
    fn value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Empty => Value::Array(Vec::new()),
            Self::Zero => Value::from(0_u8),
            Self::False => Value::Bool(false),
            Self::Configured => Value::from("configured"),
        }
    }
}

/// Completes a specimen `payload` of `producer` with the neutral value of every required field it leaves out that the neutral table names, so a specimen says only what its test is about and is still on its schema.
pub fn completed(producer: Producer, payload: &mut serde_json::Map<String, Value>) {
    let Some(kind) = payload
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return;
    };
    for neutral_record in NEUTRAL {
        if neutral_record.producer != producer || neutral_record.event != kind {
            continue;
        }
        if let Some(Value::Object(inner)) = payload.get_mut(neutral_record.record) {
            for (name, neutral) in neutral_record.fields {
                inner
                    .entry((*name).to_owned())
                    .or_insert_with(|| neutral.value());
            }
        }
    }
}
