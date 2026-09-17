// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The findings as SARIF 2.1.0, which is what a code scanning view already reads.

use std::collections::BTreeMap;

use serde::Serialize;

use super::run::{RunDocument, RunMutantDocument};

/// The version of the format this answers to.
pub const VERSION: &str = "2.1.0";

/// Where a reader of a result can read about what produced it.
pub const INFORMATION: &str = env!("CARGO_PKG_REPOSITORY");

/// The schema the version names.
pub const SCHEMA: &str = "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json";

/// The findings of one run, as SARIF.
#[derive(Debug, Clone, Serialize)]
pub struct Log {
    /// [`SCHEMA`].
    #[serde(rename = "$schema")]
    pub schema: String,
    /// [`VERSION`].
    pub version: String,
    /// One run, always.
    pub runs: Vec<Run>,
}

/// One analysis, as SARIF counts them.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// What produced it.
    pub tool: Tool,
    /// One result per finding.
    pub results: Vec<Reported>,
}

/// What produced the results.
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    /// The engine.
    pub driver: Driver,
}

/// The engine, and the rules its findings came from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Driver {
    /// `rust-mutants`.
    pub name: String,
    /// The release that answered.
    pub version: String,
    /// Where a reader can read about it.
    pub information_uri: String,
    /// The operators the findings came from.
    pub rules: Vec<ReportingDescriptor>,
}

/// One operator, as a rule a result can point at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportingDescriptor {
    /// The rule's name, which is also what a report row carries.
    pub id: String,
    /// The family it belongs to.
    pub name: String,
    /// One line about what it asks.
    pub short_description: Message,
}

/// One finding, as SARIF reports one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reported {
    /// The operator that proposed the mutation, when the finding is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// How much it matters: a survivor is a warning, a run that established nothing is an error.
    pub level: String,
    /// What a reader is told.
    pub message: Message,
    /// Where it is. Empty when the finding is not about a place in the code.
    pub locations: Vec<Location>,
    /// What makes two runs' findings the same finding.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub partial_fingerprints: BTreeMap<String, String>,
}

/// A sentence.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    /// The sentence.
    pub text: String,
}

/// Where a finding is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// The place in the file.
    pub physical_location: PhysicalLocation,
}

/// A place in a file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalLocation {
    /// The file.
    pub artifact_location: ArtifactLocation,
    /// The bytes of it.
    pub region: Region,
}

/// One file.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactLocation {
    /// The workspace-relative path.
    pub uri: String,
}

/// One stretch of one file, 1-based and end-exclusive as SARIF counts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// The 1-based line the edit starts on.
    pub start_line: u32,
    /// The 1-based column it starts at.
    pub start_column: u32,
    /// The column one past its end, when it is on one line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
    /// What the bytes are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<Message>,
}

/// The findings of one run, as a SARIF log.
#[must_use]
pub fn log(document: &RunDocument) -> Log {
    let named: BTreeMap<&str, &RunMutantDocument> = document
        .mutants
        .iter()
        .map(|mutant| (mutant.id.as_str(), mutant))
        .collect();
    let mut results = Vec::new();
    let mut rules: BTreeMap<String, ReportingDescriptor> = BTreeMap::new();
    for finding in &document.findings {
        let Some(mutant) = finding.mutant.as_deref().and_then(|id| named.get(id)) else {
            results.push(Reported {
                rule_id: None,
                level: level_of(&finding.kind).to_owned(),
                message: Message {
                    text: format!("{}: {}", finding.kind, finding.detail),
                },
                locations: Vec::new(),
                partial_fingerprints: BTreeMap::new(),
            });
            continue;
        };
        rules
            .entry(mutant.rule.clone())
            .or_insert_with(|| ReportingDescriptor {
                id: mutant.rule.clone(),
                name: mutant.family.clone(),
                short_description: Message {
                    text: format!(
                        "{}: the {} family asks what the tests say when this changes",
                        mutant.rule, mutant.family
                    ),
                },
            });
        results.push(reported(finding.kind.as_str(), &finding.detail, mutant));
    }
    Log {
        schema: SCHEMA.to_owned(),
        version: VERSION.to_owned(),
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: "rust-mutants".to_owned(),
                    version: document.tool_version.clone(),
                    information_uri: INFORMATION.to_owned(),
                    rules: rules.into_values().collect(),
                },
            },
            results,
        }],
    }
}

fn reported(kind: &str, detail: &str, mutant: &RunMutantDocument) -> Reported {
    let mut fingerprints = BTreeMap::new();
    fingerprints.insert("rustMutantsMutantId/v1".to_owned(), mutant.id.clone());
    Reported {
        rule_id: Some(mutant.rule.clone()),
        level: level_of(kind).to_owned(),
        message: Message {
            text: format!("{kind}: {detail}"),
        },
        locations: vec![Location {
            physical_location: PhysicalLocation {
                artifact_location: ArtifactLocation {
                    uri: mutant.path.clone(),
                },
                region: Region {
                    start_line: mutant.line,
                    start_column: mutant.column,
                    end_column: width_of(mutant).map(|width| mutant.column.saturating_add(width)),
                    snippet: (!mutant.original.is_empty()).then(|| Message {
                        text: mutant.original.clone(),
                    }),
                },
            },
        }],
        partial_fingerprints: fingerprints,
    }
}

/// How much one kind of finding matters.
fn level_of(kind: &str) -> &'static str {
    match kind {
        "surviving-mutant" => "warning",
        "unreached-mutant" | "unmatched-skip" | "unmatched-expectation" | "stale-expectation" => {
            "note"
        }
        _ => "error",
    }
}

/// How wide the edit is, when it does not cross a line.
fn width_of(mutant: &RunMutantDocument) -> Option<u32> {
    (!mutant.original.contains('\n'))
        .then(|| u32::try_from(mutant.original.len()).ok())
        .flatten()
}
