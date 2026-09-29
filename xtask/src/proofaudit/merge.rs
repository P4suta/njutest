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
    /// The record stream kept beside the merge says of each dimension what every part's records establish.
    Dimensions,
    /// The record stream kept beside the merge raises `unstable-baseline` about each target a part saw move that something still rests on, and states `reach-moved` about each one nothing rests on with what every part ran again against it (ADR 0036 decisions 3 and 4).
    Moved,
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
            Self::Dimensions => "dimensions",
            Self::Moved => "moved",
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
    super::Reported { path, text }: super::Reported<'_>,
    kept: Option<&str>,
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
            .fold(0, usize::max),
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
    dimensioned(&merged, kept, &mut notes);
    moved(&merged, kept, &mut notes);
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

/// Every list of `merged` a matrix is read off: what each part decided from every part, and what every part's shared baseline holds from each build's first part.
struct Lists {
    targets: Vec<Value>,
    findings: Vec<Value>,
    limitations: Vec<Value>,
    knobs: Vec<Value>,
    concurrency: Vec<Value>,
    faults: Vec<Value>,
    crashes: Vec<Value>,
    seams: Vec<Value>,
    unsettled: bool,
    found: Vec<(String, String)>,
}

/// What a merged report lacks that a matrix is read off.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unlisted {
    /// A build that holds no part.
    Build(String),
    /// A part without the list.
    List(&'static str),
    /// A row without the text at the pointer.
    Field(&'static str),
    /// A row whose evidence is in no shape a run writes.
    Evidence,
    /// Repair counts over one target that no count holds.
    Count,
}

impl Unlisted {
    /// What a violation says of it.
    fn said(&self) -> String {
        match self {
            Self::Build(name) => format!("the build {name} holds no part"),
            Self::List(name) => format!("a part holds no {name} list"),
            Self::Field(pointer) => format!("a row holds no {pointer}"),
            Self::Evidence => "a row holds evidence in no shape a run writes".to_owned(),
            Self::Count => "the parts' repair counts over one target do not fit a count".to_owned(),
        }
    }
}

impl Lists {
    /// What every part of `merged` holds, or the first thing a part does not hold.
    fn of(merged: &Complete) -> Result<Self, Unlisted> {
        let list = |part: &Value, name: &'static str| -> Result<Vec<Value>, Unlisted> {
            part.get(name)
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Unlisted::List(name))
        };
        let text = |row: &Value, pointer: &'static str| -> Result<String, Unlisted> {
            row.pointer(pointer)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or(Unlisted::Field(pointer))
        };
        let mut lists = Self {
            targets: Vec::new(),
            findings: Vec::new(),
            limitations: Vec::new(),
            knobs: Vec::new(),
            concurrency: Vec::new(),
            faults: Vec::new(),
            crashes: Vec::new(),
            seams: Vec::new(),
            unsettled: false,
            found: Vec::new(),
        };
        for build in &merged.builds {
            let baseline = build
                .parts
                .first()
                .ok_or_else(|| Unlisted::Build(build.name.clone()))?;
            lists.targets.extend(list(baseline, "targets")?);
            lists.concurrency.extend(list(baseline, "concurrency")?);
            lists.seams.extend(list(baseline, "seams")?);
            for part in &build.parts {
                lists.limitations.extend(list(part, "limitations")?);
                lists.knobs.extend(list(part, "knobs")?);
                lists.faults.extend(list(part, "faults")?);
                lists.crashes.extend(list(part, "crashes")?);
                for finding in list(part, "findings")? {
                    lists
                        .found
                        .push((text(&finding, "/kind")?, text(&finding, "/subject")?));
                    lists.findings.push(finding);
                }
                for mutant in list(part, "mutants")? {
                    let outcome = text(&mutant, "/decision/outcome")?;
                    let rests = mutant
                        .get("evidence")
                        .and_then(super::Rests::read)
                        .ok_or(Unlisted::Evidence)?;
                    let lead = matches!(rests, super::Rests::Unproven(_))
                        && super::SEALED_OUTCOMES.contains(&outcome.as_str());
                    lists.unsettled = lists.unsettled || super::undecided(&outcome, lead);
                }
            }
        }
        Ok(lists)
    }

    /// The same lists, as the matrix's re-derivation reads them.
    fn held(&self) -> super::Held<'_> {
        super::Held {
            part: super::Part {
                targets: &self.targets,
                findings: &self.findings,
                limitations: &self.limitations,
                drift: &[],
                repaired: &[],
                knobs: &self.knobs,
                concurrency: &self.concurrency,
                faults: &self.faults,
                beside: &[],
                crashes: &self.crashes,
                seams: &self.seams,
            },
            unsettled: self.unsettled,
            findings: self
                .found
                .iter()
                .map(|(kind, subject)| (kind.as_str(), subject.as_str()))
                .collect(),
        }
    }
}

/// Whether the record stream `kept` beside `merged` says of each dimension what every part's records establish, and names each one a `whole-v1` merge leaves a hole, re-derived without the runner's code (ADR 0033); a merged document stores no column, so where no stream was kept nothing was said to hold.
fn dimensioned(merged: &Complete, kept: Option<&str>, notes: &mut Notes<'_>) {
    let Some(kept) = kept else {
        return;
    };
    let lists = match Lists::of(merged) {
        Ok(lists) => lists,
        Err(why) => {
            broke(notes, MergeRule::Dimensions, "dimensions", &why.said());
            return;
        }
    };
    let holed = super::holed_dimensions(&lists.held());
    let said = super::Said::of(kept);
    for (dimension, why) in super::columns_disagree(&said, &holed) {
        broke(notes, MergeRule::Dimensions, dimension, &why);
    }
    let whole = merged.contract.as_str() == Some("whole-v1");
    for dimension in super::DIMENSIONS {
        let owed = whole && holed.contains(dimension);
        let named = said.named.contains(dimension);
        if owed != named {
            broke(
                notes,
                MergeRule::Dimensions,
                dimension,
                &format!(
                    "the record stream {} a dimension-not-measured finding about it, and a merge \
                     under {} whose parts' records {} it a hole owes {}",
                    if named { "holds" } else { "holds no" },
                    merged.contract,
                    if holed.contains(dimension) {
                        "leave"
                    } else {
                        "do not leave"
                    },
                    if owed { "one" } else { "none" }
                ),
            );
        }
    }
    for extra in said
        .named
        .iter()
        .filter(|name| !super::DIMENSIONS.contains(&name.as_str()))
    {
        broke(
            notes,
            MergeRule::Dimensions,
            extra,
            "the record stream names a dimension no matrix has",
        );
    }
}

/// What the merge owes the record stream about one target a part saw move, re-derived from every part's drift, rows and repair counts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owed {
    /// Something still rests on it: the `unstable-baseline` finding, with the words that count what does.
    Unstable(String),
    /// Nothing does: the `reach-moved` limitation, with the words that count what every part ran again.
    Repaired(String),
}

/// How many mutations, in the words a finding counts them in.
fn mutations(count: usize) -> String {
    if count == 1 {
        "1 mutation".to_owned()
    } else {
        format!("{count} mutations")
    }
}

/// Every part's records one build of a merge re-derives its moves from: the targets a part saw move, every row, and each part's repair count against each target.
struct Moves {
    moved: BTreeSet<String>,
    rows: Vec<Value>,
    repaired: Vec<(String, u64)>,
}

impl Moves {
    /// What every part of `build` holds, or the first thing a part does not hold.
    fn of(build: &Build) -> Result<Self, Unlisted> {
        let list = |part: &Value, name: &'static str| -> Result<Vec<Value>, Unlisted> {
            part.get(name)
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Unlisted::List(name))
        };
        let text = |record: &Value, name: &'static str| -> Result<String, Unlisted> {
            record
                .get(name)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or(Unlisted::Field(name))
        };
        let mut moves = Self {
            moved: BTreeSet::new(),
            rows: Vec::new(),
            repaired: Vec::new(),
        };
        for part in &build.parts {
            for record in list(part, "drift")? {
                if record.get("state").and_then(Value::as_str) == Some("moved") {
                    moves.moved.insert(text(&record, "target")?);
                }
            }
            for record in list(part, "repaired")? {
                let again = record
                    .get("again")
                    .and_then(Value::as_u64)
                    .ok_or(Unlisted::Field("again"))?;
                moves.repaired.push((text(&record, "target")?, again));
            }
            moves.rows.extend(list(part, "mutants")?);
        }
        Ok(moves)
    }

    /// How many rows came to `outcome` and rest on `target`: those whose route did not put `target` to it.
    fn resting(&self, target: &str, outcome: &str) -> usize {
        self.rows
            .iter()
            .filter(|row| row.pointer("/decision/outcome").and_then(Value::as_str) == Some(outcome))
            .filter(|row| {
                !row.pointer("/routing/reaching")
                    .and_then(Value::as_array)
                    .is_some_and(|reaching| reaching.iter().any(|one| one.as_str() == Some(target)))
            })
            .count()
    }

    /// What the merge owes about `target`, one of the moved.
    fn owed(&self, target: &str) -> Result<Owed, Unlisted> {
        let (survived, unreached) = (
            self.resting(target, "survived"),
            self.resting(target, "unreached"),
        );
        if survived > 0 || unreached > 0 {
            return Ok(Owed::Unstable(format!(
                ": {} a proof removed its run of, and {} no test reached, rest on it",
                mutations(survived),
                mutations(unreached)
            )));
        }
        let again = self
            .repaired
            .iter()
            .filter(|(named, _)| named == target)
            .try_fold(0_u64, |sum, (_, again)| sum.checked_add(*again))
            .ok_or(Unlisted::Count)?;
        Ok(Owed::Repaired(format!(
            "; {again} {} that rested on its baseline {} run again against it",
            if again == 1 {
                "disposition"
            } else {
                "dispositions"
            },
            if again == 1 { "was" } else { "were" }
        )))
    }
}

/// What the merge owes about each target a part of `merged` saw move, or what a part does not hold that it would be read from.
fn owed_moves(merged: &Complete) -> Result<Vec<(String, Owed)>, Unlisted> {
    let mut owed = Vec::new();
    for build in &merged.builds {
        let moves = Moves::of(build)?;
        for target in &moves.moved {
            owed.push((target.clone(), moves.owed(target)?));
        }
    }
    Ok(owed)
}

/// What a record stream says about moved targets: each `unstable-baseline` finding's subject and detail, and each `reach-moved` limitation's detail.
fn said_moves(kept: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut found = Vec::new();
    let mut stated = Vec::new();
    for line in kept.lines() {
        match line.split('\t').collect::<Vec<&str>>().as_slice() {
            ["FINDING", "unstable-baseline", subject, detail, ..] => {
                found.push(((*subject).to_owned(), (*detail).to_owned()));
            }
            ["LIMITATION", "reach-moved", detail, ..] => stated.push((*detail).to_owned()),
            _ => {}
        }
    }
    (found, stated)
}

/// Whether the record stream `kept` beside `merged` raises `unstable-baseline` and states `reach-moved` exactly where every part's drift, rows and repair counts decide, each counting what they count, re-derived without the runner's code (ADR 0036 decisions 3 and 4); where no stream was kept nothing was said to hold.
fn moved(merged: &Complete, kept: Option<&str>, notes: &mut Notes<'_>) {
    let Some(kept) = kept else {
        return;
    };
    let owed = match owed_moves(merged) {
        Ok(owed) => owed,
        Err(why) => {
            broke(notes, MergeRule::Moved, "moved", &why.said());
            return;
        }
    };
    let (found, stated) = said_moves(kept);
    for (target, owing) in &owed {
        held_to_owed((target, owing), (&found, &stated), notes);
    }
    for (subject, _) in &found {
        if !owed.iter().any(|(target, _)| target == subject) {
            broke(
                notes,
                MergeRule::Moved,
                subject,
                "the record stream raises unstable-baseline about it, and no part saw its reach \
                 move",
            );
        }
    }
    for detail in &stated {
        if !owed
            .iter()
            .any(|(target, _)| detail.ends_with(&format!("({target})")))
        {
            broke(
                notes,
                MergeRule::Moved,
                "reach-moved",
                &format!(
                    "the record stream states reach-moved about a target no part saw move: \
                     {detail}"
                ),
            );
        }
    }
}

/// Whether the record stream's `found` findings and `stated` limitations say of `target` exactly what `owing` owes, once, with its count, and not the other.
fn held_to_owed(
    (target, owing): (&str, &Owed),
    (found, stated): (&[(String, String)], &[String]),
    notes: &mut Notes<'_>,
) {
    let about = format!("({target})");
    let finding: Vec<&String> = found
        .iter()
        .filter(|(subject, _)| subject == target)
        .map(|(_, detail)| detail)
        .collect();
    let limitation: Vec<&String> = stated
        .iter()
        .filter(|detail| detail.ends_with(&about))
        .collect();
    let (said, counted, other, owed_name) = match owing {
        Owed::Unstable(counted) => (
            finding,
            counted,
            limitation.len(),
            "the unstable-baseline finding",
        ),
        Owed::Repaired(counted) => (
            limitation,
            counted,
            finding.len(),
            "the reach-moved limitation",
        ),
    };
    match said.as_slice() {
        [one] if one.contains(counted.as_str()) => {}
        [one] => broke(
            notes,
            MergeRule::Moved,
            target,
            &format!(
                "the record stream holds {owed_name} about it without `{}`, which every part's \
                 records count: {one}",
                counted.trim_start_matches([':', ';', ' '])
            ),
        ),
        others => broke(
            notes,
            MergeRule::Moved,
            target,
            &format!(
                "a part saw its reach move, and the record stream holds {} of {owed_name} about \
                 it where every part's records owe exactly one",
                others.len()
            ),
        ),
    }
    if other > 0 {
        broke(
            notes,
            MergeRule::Moved,
            target,
            "the record stream both raises unstable-baseline and states reach-moved about it, \
             and every part's records decide one of them",
        );
    }
}

/// A violation of `rule` about `subject`.
fn broke(notes: &mut Notes<'_>, rule: MergeRule, subject: &str, detail: &str) {
    notes.violated(subject, format!("{}: {detail}", rule.label()));
}

/// Whether the composition and the builds of `merged` are one complete division of its catalog, the merged run is none of its inputs, and it completes no model batch.
fn divided(merged: &Complete, sources: &[Source], notes: &mut Notes<'_>) {
    let Some(first) = sources.first() else {
        broke(
            notes,
            MergeRule::Division,
            "composition",
            "the report names no shard it was merged from",
        );
        return;
    };
    let of = first.shard.of;
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
        let Some(placed) = build
            .parts
            .iter()
            .map(|part| part.get("part").cloned())
            .collect::<Option<Vec<Value>>>()
        else {
            broke(
                notes,
                MergeRule::Parts,
                &build.name,
                "a part of the build does not say which shard it is",
            );
            continue;
        };
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
