// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The published schemas every recording an audit reads is held to before any reader looks at it, so no reader meets a required field that is absent.

use serde_json::Value;

/// Which producer wrote a recording, which is the schema each of its lines is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Producer {
    /// The runner, whose lines are `njutest-trace-v1`.
    Runner,
    /// The engine, whose lines are `rust-mutants-trace-v1`.
    Engine,
}

/// The canonical identifier of the engine's trace schema, which the runner's refers to.
const ENGINE_ID: &str = "https://github.com/P4suta/njutest/schema/rust-mutants-trace-v1.json";

/// The canonical identifier of the report schema, which the runner's trace refers to.
const REPORT_ID: &str = "https://github.com/P4suta/njutest/schema/njutest-assurance-report-v1.json";

/// A published schema that does not compile, which is the repository contradicting itself.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// The schema text is not JSON.
    #[error("the published {name} schema is not JSON: {source}")]
    Unparsable {
        /// Which schema.
        name: &'static str,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// The schema does not compile.
    #[error("the published {name} schema does not compile: {message}")]
    Uncompiled {
        /// Which schema.
        name: &'static str,
        /// What the validator said.
        message: String,
    },
}

/// Where one value departs from its schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at {pointer:?}")]
pub struct OffSchema {
    /// The JSON pointer of the offending value.
    pub pointer: String,
    /// What the validator said of it.
    pub message: String,
}

/// A compiled schema for one producer's lines.
#[derive(Debug)]
pub struct Checker {
    validator: jsonschema::Validator,
}

fn parsed(name: &'static str, text: &str) -> Result<Value, SchemaError> {
    crate::strictjson::from_str(text).map_err(|source| SchemaError::Unparsable { name, source })
}

fn uncompiled<E: std::fmt::Display>(name: &'static str) -> impl Fn(E) -> SchemaError {
    move |error| SchemaError::Uncompiled {
        name,
        message: error.to_string(),
    }
}

impl Checker {
    /// The schema `producer`'s lines are held to.
    ///
    /// # Errors
    /// [`SchemaError`] where a published schema does not compile.
    pub fn of(producer: Producer) -> Result<Self, SchemaError> {
        let engine = parsed(
            "engine trace",
            include_str!("../../schema/rust-mutants-trace-v1.json"),
        )?;
        let validator = match producer {
            Producer::Engine => {
                jsonschema::validator_for(&engine).map_err(uncompiled("engine trace"))?
            }
            Producer::Runner => {
                let report = parsed(
                    "report",
                    include_str!("../../schema/njutest-assurance-report-v1.json"),
                )?;
                let trace = parsed(
                    "runner trace",
                    include_str!("../../schema/njutest-trace-v1.json"),
                )?;
                let registry = jsonschema::Registry::new()
                    .add(ENGINE_ID, engine)
                    .map_err(uncompiled("engine trace"))?
                    .add(REPORT_ID, report)
                    .map_err(uncompiled("report"))?
                    .prepare()
                    .map_err(uncompiled("runner trace"))?;
                jsonschema::options()
                    .with_registry(&registry)
                    .build(&trace)
                    .map_err(uncompiled("runner trace"))?
            }
        };
        Ok(Self { validator })
    }

    /// The schema the engine's stored run report is held to.
    ///
    /// # Errors
    /// [`SchemaError`] where the published schema does not compile.
    pub fn engine_report() -> Result<Self, SchemaError> {
        let report = parsed(
            "engine run report",
            include_str!("../../schema/rust-mutants-run-report-v1.json"),
        )?;
        Ok(Self {
            validator: jsonschema::validator_for(&report)
                .map_err(uncompiled("engine run report"))?,
        })
    }

    /// Whether `value` is on its schema.
    ///
    /// # Errors
    /// The first place it departs.
    pub fn check(&self, value: &Value) -> Result<(), OffSchema> {
        match self.validator.iter_errors(value).next() {
            None => Ok(()),
            Some(error) => Err(innermost(&error)),
        }
    }
}

/// Where `error` really is: within a `oneOf` whose branches are told apart by a constant, the first error of the branch whose constant the value carries, since a reader wants the field, not the list of shapes it was not.
fn innermost(error: &jsonschema::ValidationError<'_>) -> OffSchema {
    if let jsonschema::error::ValidationErrorKind::OneOfNotValid { context } = error.kind() {
        let chosen = context.iter().find(|branch| {
            !branch.is_empty()
                && branch.iter().all(|inner| {
                    !matches!(
                        inner.kind(),
                        jsonschema::error::ValidationErrorKind::Constant { .. }
                    )
                })
        });
        if let Some(first) = chosen.and_then(|branch| branch.first()) {
            return innermost(first);
        }
    }
    OffSchema {
        pointer: error.instance_path().to_string(),
        message: error.to_string(),
    }
}

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

/// Completes a specimen `payload` of `producer` with the neutral value of every required field it leaves out and [`NEUTRAL`] names, so a specimen says only what its test is about and is still on its schema.
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
