// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Making a report comparable with the one a run made yesterday, and holding a page to opening without a network.

use std::collections::BTreeSet;

/// Every way a document can ask for something that is not in it.
pub const OUTSIDE: [&str; 10] = [
    "http://", "https://", "src=", "srcset=", "@import", "url(", "<link", "<iframe", "<object",
    "<base",
];

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
        "njutest",
        "rust_mutants",
        "workspace_digest",
        "configuration_digest",
        "root_name",
        "identity",
        "source_run_id",
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
/// The word a report uses where a fact was not available, restated here.
///
/// `njutest` states it as `report::UNAVAILABLE`, and the dependency direction `cargo xtask deps` holds keeps this crate below it rather than above, so the rule is stated again for the suites, the way `canonical` in `fixture` restates the path rule.
const UNAVAILABLE: &str = "unavailable";

/// A volatile field replaced, except where it is carrying the one thing that is not volatile.
///
/// A field is normalized because it differs between two runs of the same work.
/// The word for a fact nobody could ask for does not differ, and it is half of a pair: a document saying git could not be asked and naming a commit is one where a reader cannot tell which half to believe, and `report::Git` refuses to be read from one.
/// Blanking the sentinel made exactly that document.
fn placeholder(value: &serde_json::Value) -> serde_json::Value {
    match value.as_str() {
        None if value.is_null() => serde_json::Value::Null,
        Some(UNAVAILABLE) => value.clone(),
        None | Some(_) => serde_json::json!(PLACEHOLDER),
    }
}

/// Every way `page` asks for something that is not in it, which for a report is none.
#[must_use]
pub fn reaches_outside(page: &str) -> Vec<&'static str> {
    OUTSIDE
        .into_iter()
        .filter(|marker| page.contains(marker))
        .collect()
}
