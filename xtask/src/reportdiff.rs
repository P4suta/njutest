// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What changed between two assurance reports.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The two documents being compared, as one argument.
#[derive(Debug, Clone, Copy)]
struct Pair<'a> {
    before: &'a serde_json::Value,
    after: &'a serde_json::Value,
}

/// One thing that is not the same in both reports.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Change {
    /// What it is about, as a dotted path or a name.
    pub subject: String,
    /// What the first report said, or nothing when it did not say it.
    pub before: Option<String>,
    /// What the second said.
    pub after: Option<String>,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let before = self.before.as_deref().unwrap_or("—");
        let after = self.after.as_deref().unwrap_or("—");
        write!(f, "{}\t{before}\t{after}", self.subject)
    }
}

/// Why two reports could not be compared.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DiffError {
    /// A document is not JSON.
    #[error("{path}: not a report this version understands: {source}")]
    Unreadable {
        /// The document.
        path: String,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// A JSON document names no report kind this comparison understands.
    #[error("{path}: not a report this version understands: document_type {found}")]
    UnsupportedDocumentType {
        /// The document.
        path: String,
        /// The unknown value.
        found: String,
    },
    /// A mutation run has a mutant row whose identity or outcome cannot be compared.
    #[error("{path}: invalid run mutant row: {reason}")]
    InvalidRunMutant {
        /// The document.
        path: String,
        /// The row or field that is invalid.
        reason: String,
    },
}

impl crate::error::Coded for DiffError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::DiffUnreadable
    }
}

/// Everything that differs, in a fixed order.
///
/// # Errors
/// [`DiffError::Unreadable`] for a document that is not JSON.
/// [`DiffError::UnsupportedDocumentType`] for a document whose kind is unknown.
/// [`DiffError::InvalidRunMutant`] for a mutation run with an invalid mutant row.
pub fn compare(before: (&str, &str), after: (&str, &str)) -> Result<Vec<Change>, DiffError> {
    let left = parse(before)?;
    let right = parse(after)?;
    let (left_kind, right_kind) = (
        ReportKind::of(before.0, &left)?,
        ReportKind::of(after.0, &right)?,
    );
    if left_kind == ReportKind::Run {
        validate_run_mutants(before.0, &left)?;
    }
    if right_kind == ReportKind::Run {
        validate_run_mutants(after.0, &right)?;
    }
    let mut changes = Vec::new();

    let pair = Pair {
        before: &left,
        after: &right,
    };
    match (left_kind, right_kind) {
        (ReportKind::Run, ReportKind::Run) => compare_run(pair, &mut changes),
        (
            ReportKind::Assurance(AssuranceShape::Envelope),
            ReportKind::Assurance(AssuranceShape::Envelope),
        ) => {
            compare_tree(
                "",
                ReportLocation::Envelope,
                (Some(pair.before), Some(pair.after)),
                &mut changes,
            );
        }
        (
            ReportKind::Assurance(AssuranceShape::Bare),
            ReportKind::Assurance(AssuranceShape::Bare),
        ) => {
            compare_assurance(pair, &mut changes);
        }
        _ => compare_scalar("document_type", pair, &mut changes),
    }
    changes.sort();
    Ok(changes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportKind {
    Run,
    Assurance(AssuranceShape),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssuranceShape {
    Bare,
    Envelope,
}

impl ReportKind {
    fn of(path: &str, document: &serde_json::Value) -> Result<Self, DiffError> {
        let Some(fields) = document.as_object() else {
            return Err(DiffError::UnsupportedDocumentType {
                path: path.to_owned(),
                found: document.to_string(),
            });
        };
        match fields.get("document_type") {
            None => Ok(Self::Assurance(AssuranceShape::Bare)),
            Some(serde_json::Value::String(kind)) if kind == RUN_REPORT => Ok(Self::Run),
            Some(serde_json::Value::String(kind)) if kind == "complete" || kind == "shard" => {
                Ok(Self::Assurance(AssuranceShape::Envelope))
            }
            Some(found) => Err(DiffError::UnsupportedDocumentType {
                path: path.to_owned(),
                found: found.to_string(),
            }),
        }
    }
}

#[derive(Clone, Copy)]
enum ReportLocation {
    Envelope,
    Report,
    Builds,
    Build,
    Parts,
    Part,
    Targets,
    Target,
    Other,
}

impl ReportLocation {
    fn field(self, name: &str) -> Self {
        match (self, name) {
            (Self::Envelope, "report") => Self::Report,
            (Self::Report, "builds") => Self::Builds,
            (Self::Build, "parts") => Self::Parts,
            (Self::Part, "targets") => Self::Targets,
            _ => Self::Other,
        }
    }

    const fn element(self) -> Self {
        match self {
            Self::Builds => Self::Build,
            Self::Parts => Self::Part,
            Self::Targets => Self::Target,
            _ => Self::Other,
        }
    }

    fn volatile(self, name: &str) -> bool {
        matches!(
            (self, name),
            (Self::Report, "run_id") | (Self::Part, "timing") | (Self::Target, "duration_ms")
        )
    }
}

fn validate_run_mutants(path: &str, document: &serde_json::Value) -> Result<(), DiffError> {
    let rows = document
        .get("mutants")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| DiffError::InvalidRunMutant {
            path: path.to_owned(),
            reason: "mutants must be an array".to_owned(),
        })?;
    let mut seen = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let name = row
            .get("display_id")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| DiffError::InvalidRunMutant {
                path: path.to_owned(),
                reason: format!("mutants[{index}].display_id must be a nonempty string"),
            })?;
        if !seen.insert(name) {
            return Err(DiffError::InvalidRunMutant {
                path: path.to_owned(),
                reason: format!("mutants[{index}].display_id repeats {name:?}"),
            });
        }
        if row
            .get("outcome")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(DiffError::InvalidRunMutant {
                path: path.to_owned(),
                reason: format!("mutants[{index}].outcome must be a nonempty string"),
            });
        }
    }
    Ok(())
}

fn compare_tree(
    path: &str,
    location: ReportLocation,
    pair: (Option<&serde_json::Value>, Option<&serde_json::Value>),
    changes: &mut Vec<Change>,
) {
    match pair {
        (Some(serde_json::Value::Object(left)), Some(serde_json::Value::Object(right))) => {
            let names: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
            for name in names {
                if location.volatile(name) {
                    continue;
                }
                let at = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}.{name}")
                };
                compare_tree(
                    &at,
                    location.field(name),
                    (left.get(name), right.get(name)),
                    changes,
                );
            }
        }
        (Some(serde_json::Value::Array(left)), Some(serde_json::Value::Array(right))) => {
            for index in 0..left.len().max(right.len()) {
                compare_tree(
                    &format!("{path}[{index}]"),
                    location.element(),
                    (left.get(index), right.get(index)),
                    changes,
                );
            }
        }
        (before, after) => {
            let (before, after) = (text_of(before), text_of(after));
            if before != after {
                changes.push(Change {
                    subject: path.to_owned(),
                    before,
                    after,
                });
            }
        }
    }
}

/// What a mutation run's document is compared by.
fn compare_run(pair: Pair<'_>, changes: &mut Vec<Change>) {
    compare_flat_counts("accounting", pair, changes);
    for field in ["detected", "decided", "value"] {
        compare_under(("score", field), pair, changes);
    }
    compare_named(("findings", "mutant"), pair, changes);
    compare_named_records(("mutants", "display_id", "outcome"), pair, changes);
}

/// Every count of one top-level object of counts.
fn compare_flat_counts(group: &str, pair: Pair<'_>, changes: &mut Vec<Change>) {
    let at = |value: &serde_json::Value| {
        value
            .get(group)
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let (before, after) = (at(pair.before), at(pair.after));
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (was, is) = (text_of(before.get(name)), text_of(after.get(name)));
        if was != is {
            changes.push(Change {
                subject: format!("{group}.{name}"),
                before: was,
                after: is,
            });
        }
    }
}

/// What an assurance run's document is compared by.
fn compare_assurance(pair: Pair<'_>, changes: &mut Vec<Change>) {
    for field in ["verdict", "run_kind", "contract"] {
        compare_scalar(field, pair, changes);
    }
    for group in ["targets", "mutants", "soundness"] {
        compare_counts(group, pair, changes);
    }
    compare_named(("findings", "subject"), pair, changes);
    compare_named(("limitations", "name"), pair, changes);
    compare_named_records(("targets", "name", "status"), pair, changes);
}

/// The document type of a mutation run's report.
const RUN_REPORT: &str = "rust-mutants/run-report";

/// One value under a named object.
fn compare_under((group, field): (&str, &str), pair: Pair<'_>, changes: &mut Vec<Change>) {
    let of = |value: &serde_json::Value| text_of(value.get(group).and_then(|one| one.get(field)));
    let (before, after) = (of(pair.before), of(pair.after));
    if before != after {
        changes.push(Change {
            subject: format!("{group}.{field}"),
            before,
            after,
        });
    }
}

fn parse((path, text): (&str, &str)) -> Result<serde_json::Value, DiffError> {
    crate::strictjson::decode_str(text).map_err(|source| DiffError::Unreadable {
        path: path.to_owned(),
        source,
    })
}

/// One top-level value.
fn compare_scalar(field: &str, pair: Pair<'_>, changes: &mut Vec<Change>) {
    let (before, after) = (
        text_of(pair.before.get(field)),
        text_of(pair.after.get(field)),
    );
    if before != after {
        changes.push(Change {
            subject: field.to_owned(),
            before,
            after,
        });
    }
}

/// One group of counts under `accounting`.
fn compare_counts(group: &str, pair: Pair<'_>, changes: &mut Vec<Change>) {
    let at = |value: &serde_json::Value| {
        let mut flat = BTreeMap::new();
        if let Some(counts) = value
            .get("accounting")
            .and_then(|accounting| accounting.get(group))
        {
            flatten("", counts, &mut flat);
        }
        flat
    };
    let (before, after) = (at(pair.before), at(pair.after));
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (was, is) = (before.get(name).cloned(), after.get(name).cloned());
        if was != is {
            changes.push(Change {
                subject: format!("accounting.{group}.{name}"),
                before: was,
                after: is,
            });
        }
    }
}

/// Every count under `value`, by the dotted name a reader would use to find it.
fn flatten(prefix: &str, value: &serde_json::Value, into: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, held) in fields {
                let at = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                flatten(&at, held, into);
            }
        }
        other => {
            if let Some(text) = text_of(Some(other)) {
                into.insert(prefix.to_owned(), text);
            }
        }
    }
}

/// A list keyed by one field, compared as a set of names.
fn compare_named((list, key): (&str, &str), pair: Pair<'_>, changes: &mut Vec<Change>) {
    let (before, after) = (
        names_in(pair.before, list, key),
        names_in(pair.after, list, key),
    );
    let mut names: Vec<&String> = before.iter().chain(after.iter()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (was, is) = (before.contains(name), after.contains(name));
        if was != is {
            changes.push(Change {
                subject: format!("{list}.{name}"),
                before: was.then(|| "present".to_owned()),
                after: is.then(|| "present".to_owned()),
            });
        }
    }
}

/// A list of records keyed by one field, compared on their status.
fn compare_named_records(
    (list, key, field): (&str, &str, &str),
    pair: Pair<'_>,
    changes: &mut Vec<Change>,
) {
    let statuses = |value: &serde_json::Value| -> BTreeMap<String, String> {
        value
            .get(list)
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let name = item.get(key)?.as_str()?.to_owned();
                        let status = text_of(item.get(field))?;
                        Some((name, status))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (before, after) = (statuses(pair.before), statuses(pair.after));
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (was, is) = (before.get(name).cloned(), after.get(name).cloned());
        if was != is {
            changes.push(Change {
                subject: format!("{list}[{name}]"),
                before: was,
                after: is,
            });
        }
    }
}

fn names_in(value: &serde_json::Value, list: &str, key: &str) -> BTreeSet<String> {
    value
        .get(list)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get(key)?.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// One value as the text a reader sees, and nothing for an absent one.
fn text_of(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}
