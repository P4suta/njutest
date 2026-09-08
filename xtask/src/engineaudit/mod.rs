// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed engine run reported.
//!
//! [ADR 0004](../../../docs/adr/0004-proof-layers-not-budgets.md) ships a
//! layer only against a re-implementation that never calls the engine's, so
//! nothing here reads `rust_mutants`: every identity is re-minted from the
//! row's own fields, every column re-tallied from the rows, and every claim
//! that rests on a recording is held to that recording. Wherever a document
//! does not carry enough to re-derive a verdict, that is said plainly rather
//! than read as agreement.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod arithmetic;
mod evidence;
mod ledger;
mod recording;

use arithmetic::{accounting, exit, expectations, findings, identity, score};
use evidence::{merge, proofs, sites, touch};
use ledger::ledger;
use recording::{trace, work};

use serde_json::Value;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "run-report-v1.json";

/// The document this audit knows how to re-decide.
pub const DOCUMENT_TYPE: &str = "rust-mutants/run-report";

/// The domain separator the engine's identities are hashed under.
pub const ID_DOMAIN: &str = "rust-mutants-id-v1";

/// How many characters of an identity a person types.
pub const DISPLAY_ID_LENGTH: usize = 20;

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const TIMED_OUT: &str = "timed_out";
const INCONCLUSIVE: &str = "inconclusive";
const NOT_RUN: &str = "not_run";
const UNREACHED: &str = "unreached";
const DISCHARGED: &str = "discharged";
const UNSELECTED: &str = "unselected";
const STOPPED_EARLY: &str = "stopped-early";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNREACHED_MUTANT: &str = "unreached-mutant";
const DISCHARGED_MUTANT: &str = "discharged-mutant";
const INCONCLUSIVE_MUTANT: &str = "inconclusive-mutant";
const ERRORED_MUTANT: &str = "errored-mutant";
const NOT_RUN_MUTANT: &str = "not-run-mutant";
const STALE_EXPECTATION: &str = "stale-expectation";
const UNMATCHED_EXPECTATION: &str = "unmatched-expectation";
const MET: &str = "met";
const STALE: &str = "stale";
const UNMATCHED: &str = "unmatched";

/// Why a run could not be re-decided at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// The run directory does not hold the report a completed run leaves behind.
    #[error("{path}: a completed run leaves its report here: {source}")]
    Unreadable {
        /// Where the report was looked for.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// The document is there and is not JSON.
    #[error("{path}: not a document this audit can read: {source}")]
    Unparsable {
        /// The document.
        path: String,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// The document is JSON and calls itself something other than a run report.
    #[error("{path}: {document_type:?} is not the run report this audit re-decides")]
    Unrecognised {
        /// The document.
        path: String,
        /// What it calls itself.
        document_type: String,
    },
}

/// What the re-decision was able to conclude about one thing it looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Standing {
    /// The report contradicts itself, or rests a verdict on evidence it does not hold.
    Violated,
    /// The report does not carry what a re-decision would need, which is neither a pass nor a failure.
    Unaudited,
}

impl Standing {
    /// What to write at the head of a line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Violated => "violation",
            Self::Unaudited => "unaudited",
        }
    }
}

/// The part of a run one re-decision was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Layer {
    /// Each row's identity, re-minted from the row's own fields.
    Identity,
    /// The columns of the accounting, against the rows they summarise.
    Accounting,
    /// The score, against the columns it is a ratio over.
    Score,
    /// The correspondence between the rows and the findings that name them.
    Findings,
    /// The claims a reviewer declared, as the run left them.
    Expectations,
    /// The code the run exited with, against what it found.
    Exit,
    /// The parts of one catalog against the whole they say they are.
    Merge,
    /// The discharges the run claimed, against the evidence it kept for them.
    Proofs,
    /// Every place a rule targets, against the decision the walk took about it.
    Sites,
    /// The recording, against the report it is supposed to be the exhaust of.
    Trace,
    /// The ledger of accepted survivors, against the run that was asked to hold to it.
    Ledger,
    /// The work the report claims, against the routes it claims it from and the recording of what ran.
    Work,
    /// Every route the guards decided, re-decided from what the guards recorded.
    Touch,
}

impl Layer {
    /// Every layer, in the order they are re-decided.
    pub const ALL: [Self; 13] = [
        Self::Identity,
        Self::Accounting,
        Self::Score,
        Self::Findings,
        Self::Expectations,
        Self::Exit,
        Self::Merge,
        Self::Proofs,
        Self::Sites,
        Self::Trace,
        Self::Ledger,
        Self::Work,
        Self::Touch,
    ];

    /// What to write in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Accounting => "accounting",
            Self::Score => "score",
            Self::Findings => "findings",
            Self::Expectations => "expectations",
            Self::Exit => "exit",
            Self::Merge => "merge",
            Self::Proofs => "proofs",
            Self::Sites => "sites",
            Self::Trace => "trace",
            Self::Ledger => "ledger",
            Self::Work => "work",
            Self::Touch => "touch",
        }
    }
}

/// One thing the re-decision has to say about one part of one run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Remark {
    /// The part of the run it is about.
    pub layer: Layer,
    /// What the re-decision concluded.
    pub standing: Standing,
    /// The column, mutant, or finding it names.
    pub subject: String,
    /// One sentence a person can act on.
    pub detail: String,
}

impl fmt::Display for Remark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}: {}",
            self.standing.label(),
            self.layer.label(),
            self.subject,
            self.detail
        )
    }
}

/// What an independent re-decision made of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// The run the report names.
    pub run_id: String,
    /// How many mutant rows it re-decided over.
    pub mutants: usize,
    /// How many refused candidates it re-decided over.
    pub rejections: usize,
    /// Everything it has to say, grouped by layer with the violations of each first.
    pub remarks: Vec<Remark>,
}

impl Audit {
    /// How many things the run does not support.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.standing(Standing::Violated)
    }

    /// How many things the run does not carry enough to re-decide.
    #[must_use]
    pub fn unaudited(&self) -> usize {
        self.standing(Standing::Unaudited)
    }

    /// Every remark of one layer.
    #[must_use]
    pub fn of(&self, layer: Layer) -> Vec<&Remark> {
        self.remarks
            .iter()
            .filter(|remark| remark.layer == layer)
            .collect()
    }

    /// Whether one layer found something the run does not support.
    #[must_use]
    pub fn violated(&self, layer: Layer) -> bool {
        self.remarks
            .iter()
            .any(|remark| remark.layer == layer && remark.standing == Standing::Violated)
    }

    /// The exit code this audit earns. A run that could not be read at all never reaches here and earns [`EXIT_UNREADABLE`] instead.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        u8::from(self.violations() > 0)
    }

    fn standing(&self, standing: Standing) -> usize {
        self.remarks
            .iter()
            .filter(|remark| remark.standing == standing)
            .count()
    }
}

impl fmt::Display for Audit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for remark in &self.remarks {
            writeln!(f, "{remark}")?;
        }
        write!(
            f,
            "engine-audit: {}: {} and {} re-decided; {}, {} unaudited",
            self.run_id,
            plural(self.mutants, "mutant"),
            plural(self.rejections, "refusal"),
            plural(self.violations(), "violation"),
            self.unaudited()
        )
    }
}

/// What one run was audited against beyond its own report.
#[derive(Debug, Default)]
pub struct Evidence<'a> {
    /// The recording the run kept, when it kept one.
    pub recorded: Option<&'a str>,
    /// The reports of the other parts of this catalog, when the run was one part.
    pub shards: Vec<(String, &'a str)>,
    /// The ledger of accepted survivors, as the configuration file holds it.
    pub ledger: Option<&'a str>,
    /// Whether the census of the walk's own decisions is re-derived.
    pub sites: bool,
    /// What the coverage layer measured, as the run kept it.
    pub reached: Option<&'a str>,
    /// The catalog the run kept, which holds the body each branch proof names.
    pub catalog: Option<&'a str>,
    /// The names of the probe logs the run kept.
    pub probe_logs: Vec<String>,
    /// What the guards recorded, as the run kept it.
    pub touched: Option<&'a str>,
}

/// Where one layer's re-decisions are written down.
#[derive(Debug)]
struct Notes<'a> {
    audit: &'a mut Audit,
    layer: Layer,
}

impl<'a> Notes<'a> {
    const fn on(audit: &'a mut Audit, layer: Layer) -> Self {
        Self { audit, layer }
    }

    fn violated(&mut self, subject: &str, detail: String) {
        self.note(Standing::Violated, subject, detail);
    }

    fn unaudited(&mut self, subject: &str, detail: String) {
        self.note(Standing::Unaudited, subject, detail);
    }

    fn note(&mut self, standing: Standing, subject: &str, detail: String) {
        self.audit.remarks.push(Remark {
            layer: self.layer,
            standing,
            subject: subject.to_owned(),
            detail,
        });
    }
}

/// What an independent re-decision makes of the run report in `text`.
///
/// # Errors
/// [`AuditError::Unparsable`] for a document that is not JSON, and
/// [`AuditError::Unrecognised`] for one that is not a run report.
pub fn audit(path: &str, text: &str, evidence: &Evidence<'_>) -> Result<Audit, AuditError> {
    let document: Value = serde_json::from_str(text).map_err(|source| AuditError::Unparsable {
        path: path.to_owned(),
        source,
    })?;
    let document_type = string(&document, "document_type").unwrap_or_default();
    if document_type != DOCUMENT_TYPE {
        return Err(AuditError::Unrecognised {
            path: path.to_owned(),
            document_type,
        });
    }
    let report = Report::of(&document);
    let mut audit = Audit {
        run_id: report.run_id.clone(),
        mutants: report.mutants.len(),
        rejections: report.rejections.len(),
        remarks: Vec::new(),
    };
    identity(&report, &mut audit);
    accounting(&report, &mut audit);
    score(&report, &mut audit);
    findings(&report, &mut audit);
    expectations(&report, &mut audit);
    exit(&report, &mut audit);
    merge(&report, evidence, &mut audit);
    proofs(&report, evidence, &mut audit);
    sites(evidence, &mut audit);
    trace(&report, evidence.recorded, &mut audit);
    ledger(&report, evidence.ledger, &mut audit);
    work(&report, evidence.recorded, &mut audit);
    touch(&report, evidence.touched, &mut audit);
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// One mutant row, as a reader sees it.
#[derive(Debug, Clone)]
struct Row {
    index: Option<u64>,
    granularity: String,
    tests: BTreeMap<String, BTreeSet<String>>,
    reaching: Vec<String>,
    executed: Vec<String>,
    id: String,
    display_id: String,
    path: String,
    rule: String,
    rule_version: Option<u64>,
    start_byte: Option<u64>,
    end_byte: Option<u64>,
    source_digest: String,
    original: String,
    replacement: String,
    outcome: String,
    target: String,
    tests_run: Option<u64>,
    retried: bool,
    expected: bool,
    unreached: bool,
    not_run_reason: Option<String>,
    discharged: Vec<(String, String)>,
    source_run_id: Option<String>,
}

impl Row {
    fn label(&self) -> &str {
        if self.display_id.is_empty() {
            &self.id
        } else {
            &self.display_id
        }
    }

    /// Whether this row was not run for the named reason.
    fn not_run(&self, reason: &str) -> bool {
        self.not_run_reason.as_deref() == Some(reason)
    }

    const fn complete(&self) -> bool {
        self.rule_version.is_some()
            && self.start_byte.is_some()
            && self.end_byte.is_some()
            && !self.source_digest.is_empty()
            && !self.path.is_empty()
            && !self.rule.is_empty()
    }
}

/// One refused candidate, as a reader sees it.
#[derive(Debug, Clone)]
struct Refusal {
    index: Option<u64>,
    display_id: String,
}

/// One claim a reviewer declared, as the run left it.
#[derive(Debug, Clone)]
struct Claim {
    id: String,
    mutant: Option<String>,
    standing: String,
}

/// One thing that stops the run from being clean.
#[derive(Debug, Clone)]
struct Finding {
    kind: String,
    mutant: String,
}

/// The report, read as data.
#[derive(Debug, Default)]
struct Report {
    run_id: String,
    targets: Vec<String>,
    tool_version: String,
    workspace_digest: String,
    catalog_digest: String,
    selection: Value,
    interrupted: Option<bool>,
    exit_code: Option<u64>,
    columns: BTreeMap<String, u64>,
    score: Option<(u64, u64, f64)>,
    mutants: Vec<Row>,
    rejections: Vec<Refusal>,
    expectations: Vec<Claim>,
    findings: Vec<Finding>,
}

impl Report {
    fn of(document: &Value) -> Self {
        Self {
            run_id: document
                .get("run")
                .and_then(|run| string(run, "id"))
                .unwrap_or_default(),
            targets: document
                .get("targets")
                .and_then(Value::as_array)
                .map(|entries| entries.iter().filter_map(|one| string(one, "id")).collect())
                .unwrap_or_default(),
            tool_version: string(document, "tool_version").unwrap_or_default(),
            workspace_digest: document
                .get("workspace")
                .and_then(|it| string(it, "workspace_digest"))
                .unwrap_or_default(),
            catalog_digest: document
                .get("workspace")
                .and_then(|it| string(it, "catalog_digest"))
                .unwrap_or_default(),
            selection: document.get("selection").cloned().unwrap_or(Value::Null),
            interrupted: document
                .get("run")
                .and_then(|run| run.get("interrupted"))
                .and_then(Value::as_bool),
            exit_code: document.get("run").and_then(|run| number(run, "exit_code")),
            columns: columns(document),
            score: score_of(document),
            mutants: rows(document, "mutants").into_iter().map(row).collect(),
            rejections: rows(document, "rejections")
                .into_iter()
                .map(refusal)
                .collect(),
            expectations: rows(document, "expectations")
                .into_iter()
                .map(claim)
                .collect(),
            findings: rows(document, "findings")
                .into_iter()
                .map(finding)
                .collect(),
        }
    }

    fn counted(&self, outcome: &str) -> u64 {
        count(
            self.mutants
                .iter()
                .filter(|row| row.outcome == outcome)
                .count(),
        )
    }

    fn column(&self, name: &str) -> Option<u64> {
        self.columns.get(name).copied()
    }

    fn row(&self, subject: &str) -> Option<&Row> {
        self.mutants
            .iter()
            .find(|row| row.id == subject || row.display_id == subject)
    }
}

/// One mutant row, read as data.
fn row(value: &Value) -> Row {
    Row {
        index: number(value, "index"),
        granularity: value
            .get("route")
            .and_then(|route| string(route, "granularity"))
            .unwrap_or_default(),
        tests: tests_in(value),
        id: string(value, "id").unwrap_or_default(),
        display_id: string(value, "display_id").unwrap_or_default(),
        path: string(value, "path").unwrap_or_default(),
        rule: string(value, "rule").unwrap_or_default(),
        rule_version: number(value, "rule_version"),
        start_byte: number(value, "start_byte"),
        end_byte: number(value, "end_byte"),
        source_digest: string(value, "source_digest").unwrap_or_default(),
        original: string(value, "original").unwrap_or_default(),
        replacement: string(value, "replacement").unwrap_or_default(),
        outcome: string(value, "outcome").unwrap_or_default(),
        target: string(value, "target").unwrap_or_default(),
        tests_run: number(value, "tests_run"),
        retried: flag(value, "retried"),
        expected: flag(value, "expected"),
        unreached: flag(value, "unreached"),
        not_run_reason: string(value, "not_run_reason"),
        discharged: discharged_in(value),
        reaching: named_in(value, "reaching"),
        executed: named_in(value, "executed"),
        source_run_id: string(value, "source_run_id"),
    }
}

/// For each target one row's route narrowed to some of its tests, exactly those tests.
fn tests_in(value: &Value) -> BTreeMap<String, BTreeSet<String>> {
    value
        .get("route")
        .and_then(|route| route.get("tests"))
        .and_then(Value::as_object)
        .map(|named| {
            named
                .iter()
                .map(|(target, tests)| {
                    (
                        target.clone(),
                        tests
                            .as_array()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .filter_map(|one| one.as_str().map(ToOwned::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every target one row's route names under `field`.
fn named_in(value: &Value, field: &str) -> Vec<String> {
    value
        .get("route")
        .and_then(|route| route.get(field))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|one| one.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Every target one row says a proof removed, with the proof that removed it.
fn discharged_in(value: &Value) -> Vec<(String, String)> {
    value
        .get("route")
        .and_then(|route| route.get("discharged"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|one| Some((string(one, "target")?, string(one, "proof")?)))
                .collect()
        })
        .unwrap_or_default()
}

/// One refused candidate, read as data.
fn refusal(value: &Value) -> Refusal {
    Refusal {
        index: number(value, "index"),
        display_id: string(value, "display_id").unwrap_or_default(),
    }
}

/// One claim, read as data.
fn claim(value: &Value) -> Claim {
    Claim {
        id: string(value, "id").unwrap_or_default(),
        mutant: string(value, "mutant"),
        standing: string(value, "standing").unwrap_or_default(),
    }
}

/// One finding, read as data.
fn finding(value: &Value) -> Finding {
    Finding {
        kind: string(value, "kind").unwrap_or_default(),
        mutant: string(value, "mutant").unwrap_or_default(),
    }
}

/// Every accounting column the report carries.
fn columns(document: &Value) -> BTreeMap<String, u64> {
    document
        .get("accounting")
        .and_then(Value::as_object)
        .map(|columns| {
            columns
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_u64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// The score the report carries, when it carries one.
fn score_of(document: &Value) -> Option<(u64, u64, f64)> {
    let score = document.get("score")?;
    Some((
        number(score, "detected")?,
        number(score, "decided")?,
        score.get("value")?.as_f64()?,
    ))
}

/// One array of the document, empty when it is absent.
fn rows<'a>(document: &'a Value, key: &str) -> Vec<&'a Value> {
    array(document, key)
}

/// One array field, empty when it is absent.
fn array<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| entries.iter().collect())
        .unwrap_or_default()
}

/// One array of numbers, empty when it is absent.
fn numbers(value: &Value, key: &str) -> Vec<u64> {
    array(value, key)
        .into_iter()
        .filter_map(Value::as_u64)
        .collect()
}

/// One array of strings, empty when it is absent.
fn strings(value: &Value, key: &str) -> Vec<String> {
    array(value, key)
        .into_iter()
        .filter_map(|entry| entry.as_str().map(str::to_owned))
        .collect()
}

/// One string field, absent when it is absent or null.
fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

/// One number field, absent when it is absent or null.
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}

/// One boolean field, false when it is absent.
fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A count that never overflows the width a document uses.
fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// `n thing` or `n things`.
fn plural(count: usize, thing: &str) -> String {
    if count == 1 {
        format!("{count} {thing}")
    } else {
        format!("{count} {thing}s")
    }
}
