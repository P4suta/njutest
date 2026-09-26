// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a merged report is the shards it names: a complete division of one catalog, each part the one its re-decided shard measured, under the same run.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

use super::{Audit, AuditError, Coverage, Decided, Layer, Notes, Recorded, Remark};

/// Each thing a merged report must be of the shards it names, by which a violation says what it broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum MergeRule {
    /// The composition names one division of the catalog: one count of shards, each index once and in order, each run once.
    Division,
    /// Every configured build holds one part per shard, each saying which shard it is.
    Parts,
    /// The composition places each shard where its own document says it measured.
    Placement,
    /// The report is measured under what every shard was measured under.
    Agreement,
    /// The report holds the configured builds each shard measured, in the same order.
    Builds,
    /// Each part is, byte for byte, the part its shard measured.
    Bytes,
    /// The merged run is none of the runs it was merged from.
    Identity,
    /// A merge completes no model batch, since a contract that asks for one is refused by the merge.
    Models,
    /// Each shard, re-decided against its own recording, holds.
    Shards,
}

impl MergeRule {
    /// What a violation of it is prefixed with.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Division => "division",
            Self::Parts => "parts",
            Self::Placement => "placement",
            Self::Agreement => "agreement",
            Self::Builds => "builds",
            Self::Bytes => "bytes",
            Self::Identity => "identity",
            Self::Models => "models",
            Self::Shards => "shards",
        }
    }
}

/// One shard's identity within its catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShardPart {
    index: u64,
    of: u64,
}

/// One input a merge names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    run_id: String,
    shard: ShardPart,
}

/// How a complete report was assembled.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Composition {
    Direct,
    Merged { sources: Vec<Source> },
}

/// One configured build of a complete report.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Build {
    name: String,
    configuration: Value,
    parts: Vec<Value>,
}

/// One configured build of a shard document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShardBuild {
    name: String,
    configuration: Value,
    source: Value,
}

/// A complete report, every key of it read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    schema: String,
    schema_version: u64,
    run_id: String,
    run_kind: Value,
    contract: Value,
    tool: Value,
    repository: Value,
    provenance: Value,
    scope: Value,
    composition: Composition,
    builds: Vec<Build>,
    global_findings: Value,
    model_completion: Value,
}

/// A shard document's report, every key of it read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShardReport {
    schema: String,
    schema_version: u64,
    run_id: String,
    run_kind: Value,
    contract: Value,
    tool: Value,
    repository: Value,
    provenance: Value,
    scope: Value,
    shard: ShardPart,
    builds: Vec<ShardBuild>,
    global_findings: Value,
}

/// A document of the assurance schema, as the one closed set it is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "document_type",
    content = "report",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
enum Document {
    Complete(Box<Complete>),
    Shard(Box<ShardReport>),
}

/// A shard document re-decided against its own recording: only [`audited`] makes one, so a merge can only be held to shards that were.
#[derive(Debug)]
pub struct AuditedShard {
    report: ShardReport,
    audit: Audit,
}

/// The run a shard document at `path` holding `text` names, which is where its recording is kept.
///
/// # Errors
/// A document that is not JSON, off its schema, or not a shard.
pub fn shard_run(
    checkers: &crate::schemas::Checkers,
    path: &str,
    text: &str,
) -> Result<String, AuditError> {
    match read(checkers, path, text)? {
        Document::Shard(report) => Ok(report.run_id),
        Document::Complete(_) => Err(AuditError::NotAShard {
            path: path.to_owned(),
        }),
    }
}

/// The shard document at `path` holding `text`, re-decided against `recorded` as its one part.
///
/// # Errors
/// A document that is not a shard, and anything [`super::audit_with`] refuses.
pub fn audited(
    checkers: &crate::schemas::Checkers,
    reported: super::Reported<'_>,
    recorded: Recorded<'_>,
    run: Option<&std::path::Path>,
) -> Result<AuditedShard, AuditError> {
    let super::Reported { path, text } = reported;
    let Document::Shard(report) = read(checkers, path, text)? else {
        return Err(AuditError::NotAShard {
            path: path.to_owned(),
        });
    };
    let audit = super::audit_with(checkers, reported, recorded, run)?;
    Ok(AuditedShard {
        report: *report,
        audit,
    })
}

/// Re-decides the merged report at `path` holding `text` against the re-decided shards `shards`.
///
/// # Errors
/// A document that is not JSON or is off its schema, a report that is not a merge, a shard given twice, and a shard the report was not merged from.
pub fn merged_with(
    checkers: &crate::schemas::Checkers,
    path: &str,
    text: &str,
    shards: &[AuditedShard],
) -> Result<Audit, AuditError> {
    let Document::Complete(merged) = read(checkers, path, text)? else {
        return Err(AuditError::NotMerged {
            path: path.to_owned(),
        });
    };
    let Composition::Merged { sources } = &merged.composition else {
        return Err(AuditError::NotMerged {
            path: path.to_owned(),
        });
    };
    given(path, sources, shards)?;
    let mut audit = Audit {
        run_id: merged.run_id.clone(),
        mutants: shards.iter().map(|shard| shard.audit.mutants).sum(),
        targets: shards
            .iter()
            .map(|shard| shard.audit.targets)
            .max()
            .unwrap_or_default(),
        remarks: Vec::new(),
        coverage: std::collections::BTreeMap::new(),
    };
    let mut notes = Notes::on(&mut audit, Layer::Merge);
    divided(&merged, sources, &mut notes);
    for (position, source) in sources.iter().enumerate() {
        match shards
            .iter()
            .find(|shard| shard.report.run_id == source.run_id)
        {
            Some(shard) => held(&merged, (position, source, shard), &mut notes),
            None => notes.unaudited(
                &source.run_id,
                format!(
                    "the report was merged from {}, and its document was not given, so whether \
                     its parts are what that shard measured is not known",
                    source.run_id
                ),
            ),
        }
    }
    let Decided(()) = notes.looked();
    let whole = shards.len() == sources.len();
    for layer in Layer::ALL
        .into_iter()
        .filter(|layer| *layer != Layer::Merge)
    {
        audit.coverage.insert(
            layer,
            combined(layer, shards.iter().map(|shard| &shard.audit), whole),
        );
    }
    for shard in shards {
        for remark in &shard.audit.remarks {
            audit.remarks.push(Remark {
                subject: format!("{}: {}", shard.report.run_id, remark.subject),
                ..remark.clone()
            });
        }
    }
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// How far `layer` got over the whole catalog: re-decided only where every part was given and none fell short, and absent only where it was absent from every part.
fn combined<'a>(layer: Layer, parts: impl Iterator<Item = &'a Audit>, whole: bool) -> Coverage {
    let mut absent = None;
    let mut looked = false;
    let mut short = !whole;
    for part in parts {
        match part.coverage.get(&layer) {
            Some(Coverage::Rederived) => looked = true,
            Some(Coverage::Absent(why)) => absent = absent.or(Some(*why)),
            Some(Coverage::Partly) | None => short = true,
        }
    }
    match (short, looked, absent) {
        (true, _, _) => Coverage::Partly,
        (false, false, Some(why)) => Coverage::Absent(why),
        (false, _, _) => Coverage::Rederived,
    }
}

/// Nothing, where every shard in `shards` is given once and is one `sources` names.
fn given(path: &str, sources: &[Source], shards: &[AuditedShard]) -> Result<(), AuditError> {
    let mut seen = BTreeSet::new();
    for shard in shards {
        let run_id = shard.report.run_id.as_str();
        if !seen.insert(run_id) {
            return Err(AuditError::ShardGivenTwice {
                run_id: run_id.to_owned(),
            });
        }
        if !sources.iter().any(|source| source.run_id == run_id) {
            return Err(AuditError::ShardNotMerged {
                path: path.to_owned(),
                run_id: run_id.to_owned(),
            });
        }
    }
    Ok(())
}

/// A violation of `rule` about `subject`.
fn broke(notes: &mut Notes<'_>, rule: MergeRule, subject: &str, detail: &str) {
    notes.violated(subject, format!("{}: {detail}", rule.label()));
}

/// Whether the composition and the builds of `merged` are one complete division of its catalog, the merged run is none of its inputs, and it completes no model batch.
fn divided(merged: &Complete, sources: &[Source], notes: &mut Notes<'_>) {
    let of = sources
        .first()
        .map(|source| source.shard.of)
        .unwrap_or_default();
    let indices: Vec<u64> = sources.iter().map(|source| source.shard.index).collect();
    let expected: Vec<u64> = (1..=of).collect();
    if of == 0 || sources.iter().any(|source| source.shard.of != of) || indices != expected {
        broke(
            notes,
            MergeRule::Division,
            "composition",
            &format!(
                "the report names shards {:?}, which is not every shard of one count, once and in \
                 order",
                sources
                    .iter()
                    .map(|source| format!("{}/{}", source.shard.index, source.shard.of))
                    .collect::<Vec<_>>()
            ),
        );
    }
    let runs: BTreeSet<&str> = sources
        .iter()
        .map(|source| source.run_id.as_str())
        .collect();
    if runs.len() != sources.len() {
        broke(
            notes,
            MergeRule::Division,
            "composition",
            "the report names one run as more than one shard",
        );
    }
    placed(merged, sources, notes);
    identified(merged, &runs, notes);
}

/// Whether every build of `merged` holds one part per shard `sources` names, each saying which it is.
fn placed(merged: &Complete, sources: &[Source], notes: &mut Notes<'_>) {
    let owed: Vec<Value> = sources
        .iter()
        .map(|source| {
            serde_json::json!({
                "kind": "shard",
                "index": source.shard.index,
                "of": source.shard.of
            })
        })
        .collect();
    for build in &merged.builds {
        let placed: Vec<Value> = build
            .parts
            .iter()
            .map(|part| part.get("part").cloned().unwrap_or(Value::Null))
            .collect();
        if placed != owed {
            broke(
                notes,
                MergeRule::Parts,
                &build.name,
                &format!(
                    "the build holds parts {placed:?}, and the report was merged from {} shard(s)",
                    sources.len()
                ),
            );
        }
    }
}

/// Whether the merged run is none of the runs `runs` it names, nor any part's, and it completes no model batch.
fn identified(merged: &Complete, runs: &BTreeSet<&str>, notes: &mut Notes<'_>) {
    let evidence: BTreeSet<&str> = merged
        .builds
        .iter()
        .flat_map(|build| build.parts.iter())
        .filter_map(|part| part.get("run_id").and_then(Value::as_str))
        .collect();
    if runs.contains(merged.run_id.as_str()) || evidence.contains(merged.run_id.as_str()) {
        broke(
            notes,
            MergeRule::Identity,
            &merged.run_id,
            "the merged run names itself as one of the runs it was merged from",
        );
    }
    if merged.model_completion != serde_json::json!({ "kind": "not-required" }) {
        broke(
            notes,
            MergeRule::Models,
            "model_completion",
            "a merge completes no model batch, since it refuses a contract that asks for one",
        );
    }
}

/// One re-decided shard, at `position` in the composition as `source` names it, held to the merged report.
fn held(
    merged: &Complete,
    (position, source, shard): (usize, &Source, &AuditedShard),
    notes: &mut Notes<'_>,
) {
    let run_id = source.run_id.as_str();
    let report = &shard.report;
    if shard.audit.violations() > 0 {
        broke(
            notes,
            MergeRule::Shards,
            run_id,
            &format!(
                "re-decided against its own recording, this shard draws {} violation(s)",
                shard.audit.violations()
            ),
        );
    }
    if source.shard != report.shard {
        broke(
            notes,
            MergeRule::Placement,
            run_id,
            &format!(
                "the composition places it as shard {}/{}, and its document says it measured \
                 {}/{}",
                source.shard.index, source.shard.of, report.shard.index, report.shard.of
            ),
        );
    }
    let agreed = [
        (&merged.run_kind, &report.run_kind),
        (&merged.contract, &report.contract),
        (&merged.tool, &report.tool),
        (&merged.repository, &report.repository),
        (&merged.scope, &report.scope),
        (&merged.global_findings, &report.global_findings),
    ];
    if agreed.iter().any(|(one, other)| one != other)
        || merged.provenance.get("facts") != report.provenance.get("facts")
        || merged.schema_version != report.schema_version
        || merged.schema != "njutest-assurance-report-v1"
        || report.schema != "njutest-assurance-shard-report-v1"
    {
        broke(
            notes,
            MergeRule::Agreement,
            run_id,
            "the report is not measured under what this shard was measured under",
        );
    }
    let measured: Vec<(&str, &Value)> = report
        .builds
        .iter()
        .map(|build| (build.name.as_str(), &build.configuration))
        .collect();
    let holding: Vec<(&str, &Value)> = merged
        .builds
        .iter()
        .map(|build| (build.name.as_str(), &build.configuration))
        .collect();
    if measured != holding {
        broke(
            notes,
            MergeRule::Builds,
            run_id,
            "the report does not hold the configured builds this shard measured, in its order",
        );
    }
    for (build, source_build) in merged.builds.iter().zip(&report.builds) {
        if build.parts.get(position) != Some(&source_build.source) {
            broke(
                notes,
                MergeRule::Bytes,
                run_id,
                &format!(
                    "part {} of build {} is not the part this shard measured",
                    position.saturating_add(1),
                    build.name
                ),
            );
        }
    }
}

/// The document at `path` holding `text`, once it is JSON, on its published schema, and one of the two documents the schema describes.
fn read(
    checkers: &crate::schemas::Checkers,
    path: &str,
    text: &str,
) -> Result<Document, AuditError> {
    let value: Value =
        crate::strictjson::from_str(text).map_err(|source| AuditError::Unparsable {
            path: path.to_owned(),
            source,
        })?;
    checkers
        .assurance_report()
        .check(&value)
        .map_err(|source| AuditError::OffSchema {
            path: path.to_owned(),
            source,
        })?;
    Document::deserialize(value).map_err(|source| AuditError::Unshaped {
        path: path.to_owned(),
        source,
    })
}
