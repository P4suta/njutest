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

    /// The schema a complete assurance report is held to.
    ///
    /// # Errors
    /// [`SchemaError`] where the published schema does not compile.
    pub fn assurance_report() -> Result<Self, SchemaError> {
        let report = parsed(
            "report",
            include_str!("../../schema/njutest-assurance-report-v1.json"),
        )?;
        Ok(Self {
            validator: jsonschema::validator_for(&report).map_err(uncompiled("report"))?,
        })
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
