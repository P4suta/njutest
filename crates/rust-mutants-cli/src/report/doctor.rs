// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would find in this environment, as a document and as lines.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Names the shape of the document.
pub const DOCUMENT_TYPE: &str = "rust-mutants/doctor";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// What a run would find in this environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorDocument {
    /// [`DOCUMENT_TYPE`].
    pub document_type: String,
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The engine that answered.
    pub tool_version: String,
    /// Whether every check passed.
    pub ok: bool,
    /// Every check, in the order they were made.
    pub checks: Vec<Check>,
}

/// One thing a run needs, and whether it is here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it is as a run needs it.
    pub ok: bool,
    /// What was found.
    pub detail: String,
}

impl DoctorDocument {
    /// A document of `checks`, which is well when every one of them is.
    #[must_use]
    pub fn of(checks: Vec<Check>) -> Self {
        Self {
            document_type: DOCUMENT_TYPE.to_owned(),
            schema_version: SCHEMA_VERSION,
            tool_version: rust_mutants::VERSION.to_owned(),
            ok: checks.iter().all(|check| check.ok),
            checks,
        }
    }
}

/// The document as the lines a person reads.
#[must_use]
pub fn lines(document: &DoctorDocument) -> String {
    let mut text = String::new();
    for check in &document.checks {
        let written = writeln!(
            text,
            "{} {:<12} {}",
            if check.ok { "ok  " } else { "FAIL" },
            check.name,
            check.detail
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}
