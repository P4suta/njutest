// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Making a report comparable with the one a run made yesterday.

use std::collections::BTreeSet;

/// What a normalized field is replaced with.
pub const PLACEHOLDER: &str = "<volatile>";

/// A report with everything that changes between two runs of the same work replaced by [`PLACEHOLDER`], so what is left is what the run claimed.
#[must_use]
pub fn normalize(document: &serde_json::Value) -> serde_json::Value {
    let volatile: BTreeSet<&str> = [
        "run_id",
        "started",
        "finished",
        "commit",
        "branch",
        "merge_base",
        "rustc",
        "cargo",
        "target",
        "os",
        "arch",
        "mjutest",
        "rust_mutants",
        "workspace_digest",
        "configuration_digest",
        "provenance",
        "root_name",
    ]
    .into_iter()
    .collect();
    walk(document, &volatile)
}

/// The field a normalized report zeroes rather than blanks: a reader comparing two reports still wants the shape of the timing.
const DURATION: &str = "duration_ms";

fn walk(value: &serde_json::Value, volatile: &BTreeSet<&str>) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, field)| {
                    let replaced = if name == DURATION {
                        serde_json::json!(0)
                    } else if volatile.contains(name.as_str()) {
                        placeholder(field)
                    } else {
                        walk(field, volatile)
                    };
                    (name.clone(), replaced)
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            let mut normalized: Vec<serde_json::Value> =
                items.iter().map(|item| walk(item, volatile)).collect();
            if normalized
                .iter()
                .all(|item| item.get("id").and_then(serde_json::Value::as_str).is_some())
            {
                normalized.sort_by_key(|item| {
                    item.get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                });
            }
            serde_json::Value::Array(normalized)
        }
        other => other.clone(),
    }
}

/// The placeholder for one value, keeping `null` as `null`: "the run had no merge base" is a claim, not a moment.
fn placeholder(value: &serde_json::Value) -> serde_json::Value {
    if value.is_null() {
        serde_json::Value::Null
    } else {
        serde_json::json!(PLACEHOLDER)
    }
}
