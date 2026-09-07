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
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it is as a run needs it, which `warn` also is.
    pub ok: bool,
    /// How it stands: `ok`, `warn`, or `fail`.
    #[serde(default = "well")]
    pub status: String,
    /// What was found.
    pub detail: String,
    /// What to do about it, when it is not as a run needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

/// The standing of a check nothing said otherwise about.
fn well() -> String {
    Standing::Ok.name().to_owned()
}

/// How one check stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// It is as a run needs it.
    Ok,
    /// A run will work and something is worse than it could be.
    Warn,
    /// A run will not work.
    Fail,
}

impl Standing {
    /// The word a document and a line use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }

    /// Whether a run works with the world in this state.
    #[must_use]
    pub const fn works(self) -> bool {
        !matches!(self, Self::Fail)
    }
}

impl Check {
    /// One check, as it stands.
    #[must_use]
    pub fn new(name: &str, standing: Standing, detail: &str, remedy: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            ok: standing.works(),
            status: standing.name().to_owned(),
            detail: detail.to_owned(),
            remedy: remedy.map(ToOwned::to_owned),
        }
    }
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
            "{:<4} {:<12} {}",
            check.status.to_uppercase(),
            check.name,
            check.detail
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        if let Some(remedy) = &check.remedy {
            let written = writeln!(text, "          try: {remedy}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    text
}
