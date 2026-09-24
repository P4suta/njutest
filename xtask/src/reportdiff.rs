// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What changed between two assurance reports.

use std::collections::BTreeMap;
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
}

impl crate::error::Coded for DiffError {
    fn code(&self) -> crate::error::ErrorCode {
        crate::error::DIFF_UNREADABLE
    }
}

/// Everything that differs, in a fixed order.
///
/// # Errors
/// [`DiffError::Unreadable`] for a document that is not JSON.
pub fn compare(before: (&str, &str), after: (&str, &str)) -> Result<Vec<Change>, DiffError> {
    let left = parse(before)?;
    let right = parse(after)?;
    let mut changes = Vec::new();

    let pair = Pair {
        before: &left,
        after: &right,
    };
    if is_run_report(pair) {
        compare_run(pair, &mut changes);
    } else {
        compare_assurance(pair, &mut changes);
    }
    changes.sort();
    Ok(changes)
}

/// Whether both documents are what a mutation run writes.
fn is_run_report(pair: Pair<'_>) -> bool {
    [pair.before, pair.after].iter().all(|document| {
        document
            .get("document_type")
            .and_then(serde_json::Value::as_str)
            == Some(RUN_REPORT)
    })
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

fn names_in(
    value: &serde_json::Value,
    list: &str,
    key: &str,
) -> std::collections::BTreeSet<String> {
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
