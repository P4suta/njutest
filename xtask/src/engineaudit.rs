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

use serde_json::Value;
use sha2::{Digest as _, Sha256};

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
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNREACHED_MUTANT: &str = "unreached-mutant";
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
    /// The recording, against the report it is supposed to be the exhaust of.
    Trace,
    /// The ledger of accepted survivors, against the run that was asked to hold to it.
    Ledger,
}

impl Layer {
    /// Every layer, in the order they are re-decided.
    pub const ALL: [Self; 10] = [
        Self::Identity,
        Self::Accounting,
        Self::Score,
        Self::Findings,
        Self::Expectations,
        Self::Exit,
        Self::Merge,
        Self::Proofs,
        Self::Trace,
        Self::Ledger,
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
            Self::Trace => "trace",
            Self::Ledger => "ledger",
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
    /// What the coverage layer measured, as the run kept it.
    pub reached: Option<&'a str>,
    /// The catalog the run kept, which holds the body each branch proof names.
    pub catalog: Option<&'a str>,
    /// The names of the probe logs the run kept.
    pub probe_logs: Vec<String>,
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
    trace(&report, evidence.recorded, &mut audit);
    ledger(&report, evidence.ledger, &mut audit);
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// One mutant row, as a reader sees it.
#[derive(Debug, Clone)]
struct Row {
    index: Option<u64>,
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

/// Every identity re-minted from the row that carries it.
fn identity(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Identity);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in &report.mutants {
        if !seen.insert(&row.id) {
            notes.violated(
                row.label(),
                "two rows carry one identity; an identity that names two mutants names neither"
                    .to_owned(),
            );
        }
        if !row.complete() {
            notes.unaudited(
                row.label(),
                "the row omits what minting its identity takes, so whether it is the mutation \
                 it says it is cannot be re-derived"
                    .to_owned(),
            );
            continue;
        }
        let minted = mint(row);
        if minted != row.id {
            notes.violated(
                row.label(),
                format!(
                    "the row's own fields mint {minted} and it carries {}; a row that does not \
                     re-mint is not the mutation it says it is",
                    row.id
                ),
            );
        }
        if !row.id.starts_with(&row.display_id) || row.display_id.len() != DISPLAY_ID_LENGTH {
            notes.violated(
                row.label(),
                format!(
                    "the short identity is {} and the full one is {}; the short form is the \
                     head of the full form and nothing else",
                    row.display_id, row.id
                ),
            );
        }
    }
    dense(report, &mut notes);
}

/// The indices of the accepted and the refused together, which are the whole catalog.
fn dense(report: &Report, notes: &mut Notes<'_>) {
    let mut indices: Vec<u64> = Vec::new();
    let mut missing = false;
    for index in report
        .mutants
        .iter()
        .map(|row| row.index)
        .chain(report.rejections.iter().map(|one| one.index))
    {
        match index {
            Some(at) => indices.push(at),
            None => missing = true,
        }
    }
    if missing {
        notes.unaudited(
            "index",
            "a row carries no catalog index, so whether the catalog lost anything cannot be \
             re-derived"
                .to_owned(),
        );
        return;
    }
    indices.sort_unstable();
    let expected: Vec<u64> = (0..count(indices.len())).collect();
    if indices != expected {
        notes.violated(
            "index",
            format!(
                "the accepted and the refused carry {indices:?} where a catalog of {} runs \
                 from 0; an index that repeats or is missing means a row was lost or counted \
                 twice",
                indices.len()
            ),
        );
    }
}

/// One identity, minted the way the engine mints one.
fn mint(row: &Row) -> String {
    let mut hasher = Sha256::new();
    let version = row.rule_version.unwrap_or_default().to_string();
    let start = row.start_byte.unwrap_or_default().to_string();
    let end = row.end_byte.unwrap_or_default().to_string();
    let original = digest(row.original.as_bytes());
    let replacement = digest(row.replacement.as_bytes());
    for field in [
        ID_DOMAIN,
        &row.path,
        &row.rule,
        &version,
        &start,
        &end,
        &row.source_digest,
        &original,
        &replacement,
    ] {
        let length = u32::try_from(field.len()).unwrap_or(u32::MAX);
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// The lowercase hex SHA-256 of `bytes`.
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Every column re-tallied from the rows, and every equation the contract states.
fn accounting(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    for (name, derived) in [
        (KILLED, report.counted(KILLED)),
        (SURVIVED, report.counted(SURVIVED)),
        (TIMED_OUT, report.counted(TIMED_OUT)),
        (INCONCLUSIVE, report.counted(INCONCLUSIVE)),
        (NOT_RUN, report.counted(NOT_RUN)),
        ("cataloged", count(report.mutants.len())),
        ("refused", count(report.rejections.len())),
        (
            UNREACHED,
            count(report.mutants.iter().filter(|row| row.unreached).count()),
        ),
        (
            "expected",
            count(report.mutants.iter().filter(|row| row.expected).count()),
        ),
    ] {
        match report.column(name) {
            None => notes.unaudited(
                name,
                "the report omits this column, so the rows answer to nothing".to_owned(),
            ),
            Some(recorded) if recorded != derived => notes.violated(
                name,
                format!(
                    "the column says {recorded} and the rows come to {derived}; a report that \
                     contradicts itself is not evidence of anything"
                ),
            ),
            Some(_) => {}
        }
    }
    equations(report, &mut notes);
}

/// The equations the outcome columns stand in to each other.
fn equations(report: &Report, notes: &mut Notes<'_>) {
    let sum = [KILLED, SURVIVED, TIMED_OUT, INCONCLUSIVE, "errored"]
        .into_iter()
        .try_fold(0u64, |total, name| {
            report.column(name).map(|one| total.saturating_add(one))
        });
    match (sum, report.column("executed")) {
        (Some(outcomes), Some(executed)) if outcomes != executed => notes.violated(
            "executed",
            format!(
                "the outcome columns come to {outcomes} and the run says it executed \
                 {executed}; every execution ends in exactly one outcome"
            ),
        ),
        (None, _) | (_, None) => notes.unaudited(
            "executed",
            "the report omits a column this equation is over".to_owned(),
        ),
        _ => {}
    }
    match (
        report.column("executed"),
        report.column(NOT_RUN),
        report.column("cataloged"),
    ) {
        (Some(executed), Some(not_run), Some(cataloged))
            if executed.saturating_add(not_run) != cataloged =>
        {
            notes.violated(
                "cataloged",
                format!(
                    "{executed} executed and {not_run} not run come to \
                     {} where the catalog holds {cataloged}; every mutant is one or the other",
                    executed.saturating_add(not_run)
                ),
            );
        }
        (None, _, _) | (_, None, _) | (_, _, None) => notes.unaudited(
            "cataloged",
            "the report omits a column this equation is over".to_owned(),
        ),
        _ => {}
    }
    if let (Some(unreached), Some(not_run)) = (report.column(UNREACHED), report.column(NOT_RUN))
        && unreached > not_run
    {
        notes.violated(
            UNREACHED,
            format!(
                "{unreached} mutants are said to be unreached and only {not_run} were not run; \
                 a mutation nothing reaches is one nothing ran"
            ),
        );
    }
}

/// The score, against the columns it is a ratio over.
fn score(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Score);
    let (Some(killed), Some(timed_out), Some(survived)) = (
        report.column(KILLED),
        report.column(TIMED_OUT),
        report.column(SURVIVED),
    ) else {
        notes.unaudited(
            "score",
            "the report omits a column the score is a ratio over".to_owned(),
        );
        return;
    };
    let detected = killed.saturating_add(timed_out);
    let decided = detected.saturating_add(survived);
    match (decided > 0, report.score) {
        (false, Some(_)) => notes.violated(
            "score",
            "the run decided nothing and carries a score anyway; a score of a run that decided \
             nothing is a number about no mutants"
                .to_owned(),
        ),
        (true, None) => notes.violated(
            "score",
            format!("the run decided {decided} mutants and carries no score"),
        ),
        (true, Some((was_detected, was_decided, value))) => {
            if was_detected != detected || was_decided != decided {
                notes.violated(
                    "score",
                    format!(
                        "the score is over {was_detected} of {was_decided} and the columns come \
                         to {detected} of {decided}"
                    ),
                );
            } else if (value - ratio(detected, decided)).abs() > f64::EPSILON {
                notes.violated(
                    "score",
                    format!(
                        "the score reads {value} where {detected} of {decided} is {}",
                        ratio(detected, decided)
                    ),
                );
            }
        }
        (false, None) => {}
    }
}

/// One ratio, as the report computes it.
fn ratio(detected: u64, decided: u64) -> f64 {
    if decided == 0 {
        return 0.0;
    }
    let (detected, decided) = (
        u32::try_from(detected).unwrap_or(u32::MAX),
        u32::try_from(decided).unwrap_or(u32::MAX),
    );
    f64::from(detected) / f64::from(decided)
}

/// The findings against the rows: every kind is a set equality in both directions.
fn findings(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Findings);
    let interrupted = report.interrupted == Some(true);
    let raises = |kind: &str, row: &Row| match kind {
        SURVIVING_MUTANT => row.outcome == SURVIVED && !row.expected,
        UNREACHED_MUTANT => row.outcome == NOT_RUN && row.unreached,
        INCONCLUSIVE_MUTANT => row.outcome == INCONCLUSIVE,
        ERRORED_MUTANT => !matches!(
            row.outcome.as_str(),
            SURVIVED | INCONCLUSIVE | NOT_RUN | KILLED | TIMED_OUT
        ),
        NOT_RUN_MUTANT => row.outcome == NOT_RUN && !row.unreached && !interrupted,
        _ => false,
    };
    for kind in [
        SURVIVING_MUTANT,
        UNREACHED_MUTANT,
        INCONCLUSIVE_MUTANT,
        ERRORED_MUTANT,
        NOT_RUN_MUTANT,
    ] {
        let derived: BTreeSet<&str> = report
            .mutants
            .iter()
            .filter(|row| raises(kind, row))
            .map(Row::label)
            .collect();
        let reported: BTreeSet<&str> = report
            .findings
            .iter()
            .filter(|finding| finding.kind == kind)
            .map(|finding| {
                report
                    .row(&finding.mutant)
                    .map_or(finding.mutant.as_str(), Row::label)
            })
            .collect();
        if reported != derived {
            notes.violated(
                kind,
                format!(
                    "the rows say {derived:?} and the findings say {reported:?}; a run that \
                     does not report what it found is not a report"
                ),
            );
        }
    }
    for finding in &report.findings {
        if finding.kind == STALE_EXPECTATION || finding.kind == UNMATCHED_EXPECTATION {
            continue;
        }
        if report.row(&finding.mutant).is_none() {
            notes.violated(
                &finding.kind,
                format!(
                    "the finding names {} and no row of this run does; a finding about a \
                     mutant the run does not hold names nothing",
                    finding.mutant
                ),
            );
        }
    }
}

/// The claims a reviewer declared, as the run left them.
fn expectations(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Expectations);
    let mut met: BTreeMap<String, usize> = BTreeMap::new();
    for claim in &report.expectations {
        match claim.standing.as_str() {
            MET => match claim.mutant.as_deref().and_then(|it| report.row(it)) {
                None => notes.violated(
                    &claim.id,
                    "the claim is met and names no row of this run; a claim about a mutant \
                     that is not here was not met by anything"
                        .to_owned(),
                ),
                Some(row) => {
                    if !row.expected {
                        notes.violated(
                            &claim.id,
                            format!(
                                "the claim is met and {} is not marked expected; a row a \
                                 reviewer accounted for says so",
                                row.label()
                            ),
                        );
                    }
                    let seen = met.entry(row.label().to_owned()).or_default();
                    *seen = seen.saturating_add(1);
                }
            },
            STALE => {
                if claim
                    .mutant
                    .as_deref()
                    .and_then(|it| report.row(it))
                    .is_none()
                {
                    notes.violated(
                        &claim.id,
                        "the claim is stale and names no row of this run; a claim the run \
                         contradicted is one the run held"
                            .to_owned(),
                    );
                }
            }
            UNMATCHED => {
                if claim.mutant.is_some() {
                    notes.violated(
                        &claim.id,
                        "the claim is unmatched and names a mutant; a claim that matched \
                         nothing names nothing"
                            .to_owned(),
                    );
                }
            }
            other => notes.unaudited(
                &claim.id,
                format!("the claim stands as {other:?}, which this audit does not know"),
            ),
        }
    }
    accounted(report, &met, &mut notes);
}

/// Every row a reviewer accepted, and every claim the run contradicted, against what the findings say.
fn accounted(report: &Report, met: &BTreeMap<String, usize>, notes: &mut Notes<'_>) {
    for row in report.mutants.iter().filter(|row| row.expected) {
        match met.get(row.label()).copied().unwrap_or_default() {
            1 => {}
            0 => notes.violated(
                row.label(),
                "the row is marked expected and no claim was met by it; a row nobody \
                 accounted for is a finding"
                    .to_owned(),
            ),
            several => notes.violated(
                row.label(),
                format!("{several} claims were met by one row; a mutant is accepted once"),
            ),
        }
    }
    let standing: BTreeSet<&str> = report
        .findings
        .iter()
        .filter(|finding| finding.kind == STALE_EXPECTATION)
        .map(|finding| finding.mutant.as_str())
        .collect();
    let stale: BTreeSet<&str> = report
        .expectations
        .iter()
        .filter(|claim| claim.standing == STALE)
        .map(|claim| claim.id.as_str())
        .collect();
    if !stale.is_empty() && standing.is_empty() {
        notes.violated(
            "stale",
            format!("{stale:?} were contradicted by the run and no finding says so"),
        );
    }
}

/// The exit code, against what the run found.
fn exit(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Exit);
    let Some(recorded) = report.exit_code else {
        notes.unaudited(
            "exit_code",
            "the report omits the code the run exited with".to_owned(),
        );
        return;
    };
    let infrastructure = report
        .findings
        .iter()
        .any(|finding| finding.kind == ERRORED_MUTANT || finding.kind == NOT_RUN_MUTANT);
    let derived = if report.interrupted == Some(true) {
        130
    } else if infrastructure {
        2
    } else {
        u64::from(!report.findings.is_empty())
    };
    if recorded != derived {
        notes.violated(
            "exit_code",
            format!(
                "the run exited {recorded} and what it found earns {derived}; a script that \
                 acts on the code acts on the wrong thing"
            ),
        );
    }
}

/// The parts of one catalog, against the whole they say they are.
fn merge(report: &Report, evidence: &Evidence<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Merge);
    if evidence.shards.is_empty() {
        notes.unaudited(
            "shards",
            "no other part of this catalog was given, so whether the parts come to the whole \
             cannot be re-derived"
                .to_owned(),
        );
        return;
    }
    let mut indices: BTreeSet<u64> = report.mutants.iter().filter_map(|row| row.index).collect();
    let mut total = report.mutants.len();
    for (name, text) in &evidence.shards {
        let Ok(document) = serde_json::from_str::<Value>(text) else {
            notes.unaudited(
                name,
                "the part is not a document this audit can read".to_owned(),
            );
            continue;
        };
        let part = Report::of(&document);
        for (what, mine, theirs) in [
            (
                "catalog_digest",
                &report.catalog_digest,
                &part.catalog_digest,
            ),
            (
                "workspace_digest",
                &report.workspace_digest,
                &part.workspace_digest,
            ),
            ("tool_version", &report.tool_version, &part.tool_version),
        ] {
            if mine != theirs {
                notes.violated(
                    name,
                    format!(
                        "the part's {what} is {theirs} and this run's is {mine}; parts of one \
                         catalog answer the same question"
                    ),
                );
            }
        }
        if part.selection != report.selection {
            notes.violated(
                name,
                "the part asked for something else; parts of one catalog were asked the same \
                 thing"
                    .to_owned(),
            );
        }
        total = total.saturating_add(part.mutants.len());
        for row in &part.mutants {
            if let Some(index) = row.index
                && !indices.insert(index)
            {
                notes.violated(
                    name,
                    format!("index {index} is in two parts; a mutant belongs to one part"),
                );
            }
        }
    }
    if indices.len() != total {
        notes.violated(
            "shards",
            format!(
                "the parts hold {total} rows over {} distinct indices; a part that repeats a \
                 mutant counts it twice",
                indices.len()
            ),
        );
    }
}

/// The recording, against the report it is supposed to be the exhaust of.
fn trace(report: &Report, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Trace);
    let Some(recorded) = recorded else {
        notes.unaudited(
            "recording",
            "the run kept no recording, so what it did cannot be held to what it reported"
                .to_owned(),
        );
        return;
    };
    let events = events(recorded);
    complete(&events, &mut notes);
    instrumented(&events, &mut notes);
    verified(&events, &mut notes);
    condemned(report, &events, &mut notes);
    routed(report, recorded, &mut notes);
}

/// Whether the recording begins, ends, lost nothing, and closed every phase it opened.
fn complete(events: &[Value], notes: &mut Notes<'_>) {
    let kind = |event: &Value| string(event, "type").unwrap_or_default();
    match events.first() {
        None => {
            notes.unaudited("recording", "the recording holds no event".to_owned());
            return;
        }
        Some(first) if kind(first) != "run-start" => notes.violated(
            "run-start",
            "the recording does not begin with run-start; its beginning was lost".to_owned(),
        ),
        Some(_) => {}
    }
    let mut expected = events
        .first()
        .and_then(|first| number(first, "seq"))
        .unwrap_or(1);
    for event in events {
        let seq = number(event, "seq").unwrap_or_default();
        if seq != expected {
            notes.violated(
                "seq",
                format!("sequence {expected} is missing and the next event is {seq}; the sink lost what was between"),
            );
        }
        expected = seq.saturating_add(1);
    }
    let mut open: Vec<String> = Vec::new();
    for event in events {
        match kind(event).as_str() {
            "phase-start" => open.push(phase_name(event)),
            "phase-end" => {
                let name = phase_name(event);
                if let Some(at) = open.iter().rposition(|held| *held == name) {
                    let _closed = open.remove(at);
                } else {
                    let said = format!("the phase {name} ended without beginning");
                    notes.violated("phase", said);
                }
            }
            _ => {}
        }
    }
    for name in open {
        notes.violated("phase", format!("the phase {name} began and never ended"));
    }
    match events.last() {
        Some(last) if kind(last) == "run-end" => {
            let dropped = last
                .get("run")
                .and_then(|run| number(run, "events_dropped"))
                .unwrap_or_default();
            if dropped > 0 {
                notes.violated(
                    "run-end",
                    format!(
                        "the run admits it dropped {dropped} events; what it did is not all here"
                    ),
                );
            }
        }
        _ => notes.violated(
            "run-end",
            "the recording does not end with run-end; the run was killed, or the end was lost"
                .to_owned(),
        ),
    }
}

/// Every instrumentation, held to the one thing it must not change.
fn instrumented(events: &[Value], notes: &mut Notes<'_>) {
    for event in events {
        if string(event, "type").as_deref() != Some("instrument") {
            continue;
        }
        let Some(record) = event.get("instrument") else {
            continue;
        };
        let (before, after) = (
            number(record, "lines_before"),
            number(record, "lines_after"),
        );
        let path = string(record, "path").unwrap_or_default();
        match (before, after) {
            (Some(before), Some(after)) if before != after => notes.violated(
                &path,
                format!(
                    "instrumenting moved the file from {before} lines to {after}; a position \
                     the report names would be a position in a file nobody has"
                ),
            ),
            (None, _) | (_, None) => notes.unaudited(
                &path,
                "the record omits the line counts, so whether instrumenting moved a line \
                 cannot be re-derived"
                    .to_owned(),
            ),
            _ => {}
        }
    }
}

/// Every target the build produced, against the verification of it.
fn verified(events: &[Value], notes: &mut Notes<'_>) {
    let mut built: BTreeSet<String> = BTreeSet::new();
    let mut verified: BTreeSet<String> = BTreeSet::new();
    for event in events {
        match string(event, "type").as_deref() {
            Some("build") => {
                if let Some(record) = event.get("build") {
                    built.extend(strings(record, "targets"));
                }
            }
            Some("verify") => {
                if let Some(record) = event.get("verify")
                    && let Some(target) = string(record, "target")
                {
                    verified.insert(target);
                }
            }
            _ => {}
        }
    }
    if verified.is_empty() {
        notes.unaudited(
            "verify",
            "nothing was verified, so what the suite says with nothing active is not on record"
                .to_owned(),
        );
        return;
    }
    for target in built.difference(&verified) {
        notes.violated(
            target,
            "the build produced this target and nothing verified it; an outcome from a target \
             whose baseline nobody read is not about the mutation"
                .to_owned(),
        );
    }
}

/// The refusals, against the rounds that condemned them.
fn condemned(report: &Report, events: &[Value], notes: &mut Notes<'_>) {
    let mut named: BTreeSet<u64> = BTreeSet::new();
    let mut rounds = 0usize;
    for event in events {
        match string(event, "type").as_deref() {
            Some("validate-round") => {
                rounds = rounds.saturating_add(1);
                if let Some(record) = event.get("round") {
                    for one in array(record, "attributed") {
                        if let Some(index) = number(one, "index") {
                            named.insert(index);
                        }
                    }
                }
            }
            Some("bisect") => {
                if let Some(record) = event.get("bisect") {
                    named.extend(numbers(record, "offenders"));
                }
            }
            _ => {}
        }
    }
    if rounds == 0 {
        if !report.rejections.is_empty() {
            notes.unaudited(
                "rejections",
                "the recording holds no validation round and the report holds refusals, so \
                 what condemned each candidate cannot be re-derived"
                    .to_owned(),
            );
        }
        return;
    }
    let refused: BTreeSet<u64> = report
        .rejections
        .iter()
        .filter_map(|one| one.index)
        .collect();
    if refused.len() != report.rejections.len() {
        notes.unaudited(
            "rejections",
            "a refusal carries no catalog index, so what condemned it cannot be re-derived"
                .to_owned(),
        );
        return;
    }
    for index in named.difference(&refused) {
        notes.violated(
            &index.to_string(),
            "a validation round condemned this candidate and the report does not refuse it; a \
             candidate the compiler refused is one the catalog does not hold"
                .to_owned(),
        );
    }
    for index in refused.difference(&named) {
        let subject = report
            .rejections
            .iter()
            .find(|one| one.index == Some(*index))
            .map_or_else(|| index.to_string(), |one| one.display_id.clone());
        notes.violated(
            &subject,
            "the report refuses this candidate and no round condemned it; a refusal nothing \
             accounts for is a mutant somebody dropped"
                .to_owned(),
        );
    }
}

/// Every row against the route and the executions the recording holds for it.
fn routed(report: &Report, recorded: &str, notes: &mut Notes<'_>) {
    let routing = crate::route::read(recorded);
    if routing.routes.is_empty() && routing.execs.is_empty() {
        notes.unaudited(
            "route",
            "the recording holds no routing decision and no execution, so nothing holds a row \
             to what it did"
                .to_owned(),
        );
        return;
    }
    let reused = report
        .mutants
        .iter()
        .filter(|row| row.source_run_id.is_some())
        .count();
    if reused > 0 {
        notes.unaudited(
            "reuse",
            format!(
                "{reused} rows were read back from an earlier run, so this recording says \
                 nothing about how they were decided"
            ),
        );
    }
    let stopped = report
        .mutants
        .iter()
        .filter(|row| row.outcome == NOT_RUN && report.interrupted == Some(true))
        .count();
    if stopped > 0 {
        notes.unaudited(
            "interrupted",
            format!(
                "{stopped} rows were never reached because the run was stopped, so there is \
                 nothing about them to hold the recording to"
            ),
        );
    }
    for row in &report.mutants {
        if row.source_run_id.is_some()
            || (row.outcome == NOT_RUN && report.interrupted == Some(true))
        {
            continue;
        }
        let Some(route) = routing.route_of(&row.id, &row.display_id) else {
            notes.violated(
                row.label(),
                "the report judges this mutant and the recording holds no route for it; a \
                 decision nobody recorded is one nobody can check"
                    .to_owned(),
            );
            continue;
        };
        let execs: Vec<&crate::route::Exec> = routing.execs_for(&row.id, &row.display_id).collect();
        answered(row, &execs, notes);
        reached(row, route, &execs, notes);
        discharged(row, route, &execs, notes);
    }
}

/// The row's own answer, against the execution of the target it names.
///
/// A run walks the targets a route holds and stops at the first that detects,
/// so the target a row names is the one whose answer it carries and not
/// necessarily the last one that ran: a target that ran nothing says nothing,
/// and the answer comes from the one before it.
fn answered(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    if execs.is_empty() {
        return;
    }
    let Some(answer) = execs.iter().rev().find(|exec| exec.target == row.target) else {
        notes.violated(
            row.label(),
            format!(
                "the row says {} answered and the recording holds no execution against it; a \
                 report that names a target its own recording did not run is not evidence",
                row.target
            ),
        );
        return;
    };
    if answer.outcome != row.outcome {
        notes.violated(
            row.label(),
            format!(
                "the row says {} and its execution against {} says {}; a report that disagrees \
                 with its own recording is not evidence",
                row.outcome, row.target, answer.outcome
            ),
        );
    }
    if let (Some(recorded), Some(reported)) = (answer.tests_run, row.tests_run)
        && recorded != reported
    {
        notes.violated(
            row.label(),
            format!("the row says {reported} tests ran and the execution says {recorded}"),
        );
    }
    retried(row, execs, notes);
}

/// A timeout the run believed, against the retry that is what believing one takes.
fn retried(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    let timed_out = execs
        .iter()
        .filter(|exec| exec.outcome == TIMED_OUT)
        .count();
    if row.outcome == TIMED_OUT && (timed_out < 2 || !row.retried) {
        notes.violated(
            row.label(),
            format!(
                "the row is a timeout and the recording holds {timed_out} of them with \
                 retried={}; a timeout is believed only after it repeats on its own",
                row.retried
            ),
        );
    }
    if row.retried && timed_out == 0 {
        notes.violated(
            row.label(),
            "the row says it was retried and nothing timed out; a retry is what a timeout \
             costs and nothing else asks for one"
                .to_owned(),
        );
    }
    if row.outcome == INCONCLUSIVE && row.retried && timed_out < 1 {
        notes.violated(
            row.label(),
            "the row could not be decided after a retry and nothing timed out; inconclusive \
             after a retry is what a timeout that did not repeat leaves behind"
                .to_owned(),
        );
    }
}

/// The route's granularity, against whether anything ran.
fn reached(
    row: &Row,
    route: &crate::route::Route,
    execs: &[&crate::route::Exec],
    notes: &mut Notes<'_>,
) {
    match (route.granularity.as_str(), execs.is_empty()) {
        (UNREACHED, false) => notes.violated(
            row.label(),
            "the route says no measured target reaches this mutation and the recording then \
             runs one against it; a claim its own run contradicts is not a claim"
                .to_owned(),
        ),
        ("all" | "block", true) if row.outcome != NOT_RUN => notes.violated(
            row.label(),
            format!(
                "the route says {} targets could notice this mutation and nothing ran; an \
                 outcome of {} rests on an execution the recording does not hold",
                route.reaching.len(),
                row.outcome
            ),
        ),
        _ => {}
    }
    if row.unreached && route.granularity != UNREACHED {
        notes.violated(
            row.label(),
            format!(
                "the row says nothing reaches this mutation and the route says {}",
                route.granularity
            ),
        );
    }
}

/// Every target a proof removed, against the executions of the mutant it was removed from.
fn discharged(
    row: &Row,
    route: &crate::route::Route,
    execs: &[&crate::route::Exec],
    notes: &mut Notes<'_>,
) {
    for exec in execs {
        if route.discharges(&exec.target) {
            notes.violated(
                row.label(),
                format!(
                    "a proof removed {} from what could notice this mutation and it then ran \
                     against it; a layer that removes work is not a layer that also does it",
                    exec.target
                ),
            );
        }
    }
}

/// The ledger of accepted survivors, against the run that was asked to hold to it.
fn ledger(report: &Report, ledger: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Ledger);
    let Some(ledger) = ledger else {
        notes.unaudited(
            "ledger",
            "no ledger was given, so whether every survivor is one somebody accepted cannot be \
             re-decided"
                .to_owned(),
        );
        return;
    };
    let entries = accepted(ledger);
    for finding in &report.findings {
        if finding.kind == SURVIVING_MUTANT || finding.kind == UNREACHED_MUTANT {
            notes.violated(
                &finding.mutant,
                "no test noticed this mutation and the ledger does not accept it; a survivor \
                 is either killed or accepted with a reason"
                    .to_owned(),
            );
        }
    }
    let standing: BTreeSet<&str> = report
        .expectations
        .iter()
        .map(|claim| claim.id.as_str())
        .collect();
    for entry in &entries {
        if !standing.contains(entry.as_str()) {
            notes.violated(
                entry,
                "the ledger accepts this mutant and the run does not hold it; an acceptance \
                 nothing answers is one nobody will notice going stale"
                    .to_owned(),
            );
        }
    }
    for claim in report.expectations.iter().filter(|it| it.standing == MET) {
        if !entries.contains(&claim.id) {
            notes.violated(
                &claim.id,
                "the run met a claim the ledger does not carry; an acceptance a reviewer \
                 cannot find is not one they made"
                    .to_owned(),
            );
        }
    }
}

/// Every mutant the ledger accepts, by the identity it names.
fn accepted(ledger: &str) -> BTreeSet<String> {
    let Ok(document) = ledger.parse::<toml::Table>() else {
        return BTreeSet::new();
    };
    document
        .get("mutation")
        .and_then(toml::Value::as_table)
        .and_then(|mutation| mutation.get("expect"))
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id")?.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Every event of a recording, skipping what is not one.
fn events(recorded: &str) -> Vec<Value> {
    recorded
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// The name of the phase one boundary is about.
fn phase_name(event: &Value) -> String {
    event
        .get("phase")
        .and_then(|phase| string(phase, "name"))
        .unwrap_or_default()
}

/// One mutant row, read as data.
fn row(value: &Value) -> Row {
    Row {
        index: number(value, "index"),
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
        source_run_id: string(value, "source_run_id"),
    }
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

/// Re-derives every discharge the run claimed from the evidence it kept.
///
/// A discharge removes an execution, so a report that names one without the
/// premises is a claim rather than a proof. This reads the measurement and
/// the catalog the run kept, re-implements the rule — a target whose covered
/// regions begin nowhere inside the body a branch proof names cannot have
/// noticed the mutation — and says whether the run's own answer follows.
fn proofs(report: &Report, evidence: &Evidence<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let discharged = report
        .mutants
        .iter()
        .filter(|row| row.not_run_reason.as_deref() == Some("discharged"))
        .count();
    let counted = report
        .columns
        .get("discharged")
        .copied()
        .unwrap_or_default();
    if u64::try_from(discharged).unwrap_or(u64::MAX) != counted {
        notes.violated(
            "discharged",
            format!(
                "the accounting says {counted} mutants were discharged and {discharged} rows \
                 say so"
            ),
        );
    }
    if let Some(recorded) = evidence.recorded {
        selected(report, recorded, &mut notes);
    }
    let claims = claimed(report);
    if claims.is_empty() {
        notes.unaudited(
            "discharge",
            "the run discharged nothing, so there is no proof to re-derive".to_owned(),
        );
        return;
    }
    branch_discharges(&claims, evidence, &mut notes);
    infection_discharges(&claims, evidence, &mut notes);
    if let Some(recorded) = evidence.recorded {
        never_ran(&claims, recorded, &mut notes);
    }
}

/// Every mutant that never ran says why, in the recording as well as in the report.
fn selected(report: &Report, recorded: &str, notes: &mut Notes<'_>) {
    let said: BTreeMap<String, String> = events(recorded)
        .iter()
        .filter(|event| string(event, "type").as_deref() == Some("select"))
        .filter_map(|event| {
            let select = event.get("select")?;
            Some((string(select, "mutant")?, string(select, "reason")?))
        })
        .collect();
    for row in report
        .mutants
        .iter()
        .filter(|row| row.outcome == NOT_RUN && row.source_run_id.is_none())
    {
        let Some(reason) = said.get(&row.display_id) else {
            notes.violated(
                "select",
                format!(
                    "{} never ran and the recording does not say why",
                    row.label()
                ),
            );
            continue;
        };
        if row.not_run_reason.as_deref() != Some(reason.as_str()) {
            notes.violated(
                "select",
                format!(
                    "{} is {} in the report and {reason} in the recording",
                    row.label(),
                    row.not_run_reason.as_deref().unwrap_or("unexplained")
                ),
            );
        }
    }
}

/// One discharge a report claims: which mutant, which target, and which proof.
struct Discharged {
    mutant: String,
    target: String,
    proof: String,
}

/// Every discharge the report's own routes name.
fn claimed(report: &Report) -> Vec<Discharged> {
    report
        .mutants
        .iter()
        .flat_map(|row| {
            row.discharged.iter().map(|(target, proof)| Discharged {
                mutant: row.display_id.clone(),
                target: target.clone(),
                proof: proof.clone(),
            })
        })
        .collect()
}

/// Re-derives every `branch-never-taken` discharge from the measurement and the body the catalog names.
fn branch_discharges(claims: &[Discharged], evidence: &Evidence<'_>, notes: &mut Notes<'_>) {
    let branch: Vec<&Discharged> = claims
        .iter()
        .filter(|claim| claim.proof == "branch-never-taken")
        .collect();
    if branch.is_empty() {
        return;
    }
    let (Some(reached), Some(catalog)) = (evidence.reached, evidence.catalog) else {
        notes.unaudited(
            "branch-never-taken",
            format!(
                "{} discharges rest on a measurement and a catalog the run did not keep",
                branch.len()
            ),
        );
        return;
    };
    let Ok(reached) = serde_json::from_str::<Value>(reached) else {
        notes.unaudited(
            "branch-never-taken",
            "the measurement the run kept is not a document".to_owned(),
        );
        return;
    };
    let Ok(catalog) = serde_json::from_str::<Value>(catalog) else {
        notes.unaudited(
            "branch-never-taken",
            "the catalog the run kept is not a document".to_owned(),
        );
        return;
    };
    for claim in branch {
        let Some(row) = mutant_row(&catalog, &claim.mutant) else {
            notes.unaudited(
                "branch-never-taken",
                format!("the catalog holds no row for {}", claim.mutant),
            );
            continue;
        };
        let Some(body) = row.get("branch") else {
            notes.violated(
                "branch-never-taken",
                format!(
                    "{} was discharged from {} by a branch proof the catalog does not hold",
                    claim.mutant, claim.target
                ),
            );
            continue;
        };
        let path = string(row, "path").unwrap_or_default();
        if ran_the_body(&reached, &claim.target, &path, body) {
            notes.violated(
                "branch-never-taken",
                format!(
                    "{} covered a region inside the body {} sits in, so it may have noticed it",
                    claim.target, claim.mutant
                ),
            );
        }
    }
}

/// The catalog row of one mutant, by the identity a report names it with.
fn mutant_row<'a>(catalog: &'a Value, display_id: &str) -> Option<&'a Value> {
    catalog
        .get("mutants")?
        .as_array()?
        .iter()
        .find(|row| string(row, "display_id").as_deref() == Some(display_id))
}

/// Whether the target's measured run covered a region beginning inside the body.
fn ran_the_body(reached: &Value, target: &str, path: &str, body: &Value) -> bool {
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let start = (number(body, "start_line"), number(body, "start_column"));
    let end = (number(body, "end_line"), number(body, "end_column"));
    let Some(blocks) = reached
        .get("targets")
        .and_then(|targets| targets.get(target))
        .and_then(Value::as_array)
    else {
        return false;
    };
    blocks.iter().any(|block| {
        if string(block, "file").as_deref() != Some(path) {
            return false;
        }
        let Some(at) = block.get("start") else {
            return false;
        };
        let position = (number(at, "line"), number(at, "column"));
        position >= start && position < end
    })
}

/// Every `never-infected` discharge rests on a log the run kept.
fn infection_discharges(claims: &[Discharged], evidence: &Evidence<'_>, notes: &mut Notes<'_>) {
    for claim in claims
        .iter()
        .filter(|claim| claim.proof == "never-infected")
    {
        let wanted = format!("{}.log", claim.target.replace('/', "-"));
        if !evidence.probe_logs.iter().any(|name| name == &wanted) {
            notes.unaudited(
                "never-infected",
                format!(
                    "{} was discharged from {} by a probe whose log the run did not keep",
                    claim.mutant, claim.target
                ),
            );
        }
    }
}

/// A discharged pair that then ran is a proof the run contradicted.
fn never_ran(claims: &[Discharged], recorded: &str, notes: &mut Notes<'_>) {
    let routing = crate::route::read(recorded);
    for claim in claims {
        if routing
            .execs
            .iter()
            .any(|exec| exec.mutant == claim.mutant && exec.target == claim.target)
        {
            notes.violated(
                "discharge",
                format!(
                    "{} was discharged from {} and then executed against it",
                    claim.mutant, claim.target
                ),
            );
        }
    }
}
