// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a merged report is the shards it names: each part the one its shard measured, under the same run, in the place its composition gives it.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Audit, AuditError, Layer, Notes, field, rows};

/// The fields every shard of one merge was measured under, which the merged report carries once.
const AGREED: [&str; 6] = [
    "run_kind",
    "contract",
    "tool",
    "repository",
    "scope",
    "global_findings",
];

/// Re-decides the merged report at `path` holding `text` against the shard documents `shards`, each its path and its text.
///
/// # Errors
/// A document that is not JSON or is off its published schema, a report that is not a merge, a document given as a shard that is not one, and a shard the report was not merged from.
pub fn merged_with(
    path: &str,
    text: &str,
    shards: &[(String, String)],
) -> Result<Audit, AuditError> {
    let merged = checked(path, text)?;
    let report = merged.get("report").cloned().unwrap_or_default();
    let composition = report.get("composition").cloned().unwrap_or_default();
    if field(&merged, "document_type").as_deref() != Some("complete")
        || field(&composition, "kind").as_deref() != Some("merged")
    {
        return Err(AuditError::NotMerged {
            path: path.to_owned(),
        });
    }
    let sources = rows(&composition, "sources");
    let given = given(sources, shards)?;
    let builds = rows(&report, "builds");
    let mut audit = Audit {
        run_id: field(&report, "run_id").unwrap_or_default(),
        mutants: builds
            .first()
            .map(|build| {
                rows(build, "parts")
                    .iter()
                    .map(|part| rows(part, "mutants").len())
                    .sum()
            })
            .unwrap_or_default(),
        targets: builds
            .first()
            .and_then(|build| rows(build, "parts").first())
            .map(|part| rows(part, "targets").len())
            .unwrap_or_default(),
        remarks: Vec::new(),
    };
    let mut notes = Notes::on(&mut audit, Layer::Merge);
    for build in builds {
        let parts = rows(build, "parts").len();
        if parts != sources.len() {
            notes.violated(
                "composition",
                format!(
                    "the report was merged from {} shard(s), and build {} holds {parts} part(s)",
                    sources.len(),
                    field(build, "name").unwrap_or_default()
                ),
            );
        }
    }
    for (position, source) in sources.iter().enumerate() {
        let run_id = field(source, "run_id").unwrap_or_default();
        match given.get(&run_id) {
            Some(shard) => held(&report, (position, source, shard), &mut notes),
            None => notes.unaudited(
                &run_id,
                format!(
                    "the report was merged from {run_id}, and its document was not given, so \
                     whether its parts are what that shard measured is not known"
                ),
            ),
        }
    }
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// Every shard document in `shards` by the run it names, each on its schema and one the composition's `sources` name.
fn given(
    sources: &[Value],
    shards: &[(String, String)],
) -> Result<BTreeMap<String, Value>, AuditError> {
    let mut given = BTreeMap::new();
    for (shard_path, shard_text) in shards {
        let shard = checked(shard_path, shard_text)?;
        if field(&shard, "document_type").as_deref() != Some("shard") {
            return Err(AuditError::NotAShard {
                path: shard_path.clone(),
            });
        }
        let shard_report = shard.get("report").cloned().unwrap_or_default();
        let run_id = field(&shard_report, "run_id").unwrap_or_default();
        if !sources
            .iter()
            .any(|source| field(source, "run_id").as_deref() == Some(run_id.as_str()))
        {
            return Err(AuditError::ShardNotMerged {
                path: shard_path.clone(),
                run_id,
            });
        }
        given.insert(run_id, shard_report);
    }
    Ok(given)
}

/// One shard, at `position` in the composition as `source` names it, held to the merged `report`.
fn held(report: &Value, (position, source, shard): (usize, &Value, &Value), notes: &mut Notes<'_>) {
    let run_id = field(source, "run_id").unwrap_or_default();
    if source.get("shard") != shard.get("shard") {
        notes.violated(
            &run_id,
            format!(
                "the composition places it as shard {}, and its document says it measured {}",
                source.get("shard").cloned().unwrap_or_default(),
                shard.get("shard").cloned().unwrap_or_default()
            ),
        );
    }
    for key in AGREED {
        if report.get(key) != shard.get(key) {
            notes.violated(
                &run_id,
                format!("the report's {key} is not the one this shard was measured under"),
            );
        }
    }
    let merged = rows(report, "builds");
    let measured = rows(shard, "builds");
    if merged.len() != measured.len() {
        notes.violated(
            &run_id,
            format!(
                "the report holds {} configured build(s), and this shard measured {}",
                merged.len(),
                measured.len()
            ),
        );
    }
    for (build, source_build) in merged.iter().zip(measured) {
        let name = field(build, "name").unwrap_or_default();
        if build.get("name") != source_build.get("name")
            || build.get("configuration") != source_build.get("configuration")
        {
            notes.violated(
                &run_id,
                format!(
                    "build {name} is not the configured build this shard measured in its place"
                ),
            );
        }
        if rows(build, "parts").get(position) != source_build.get("source") {
            notes.violated(
                &run_id,
                format!(
                    "part {} of build {name} is not the part this shard measured",
                    position.saturating_add(1)
                ),
            );
        }
    }
}

/// The document at `path` holding `text`, once it is JSON and on its published schema.
fn checked(path: &str, text: &str) -> Result<Value, AuditError> {
    let read: Value =
        crate::strictjson::from_str(text).map_err(|source| AuditError::Unparsable {
            path: path.to_owned(),
            source,
        })?;
    crate::schemas::Checker::assurance_report()?
        .check(&read)
        .map_err(|source| AuditError::OffSchema {
            path: path.to_owned(),
            source,
        })?;
    Ok(read)
}
