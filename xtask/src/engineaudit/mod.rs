// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed engine run reported.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod arithmetic;
mod evidence;
mod ledger;
mod recording;
pub mod sentinel;
mod wire;

use arithmetic::{accounting, exit, expectations, findings, identity, score};
use evidence::{entry, merge, proofs, sites, touch};
use ledger::ledger;
use recording::{trace, work};

use serde_json::Value;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "run-report-v1.json";

/// The document this audit knows how to re-decide.
pub const DOCUMENT_TYPE: &str = "rust-mutants/run-report";

/// The current report shape this audit independently re-decides.
pub const SCHEMA_VERSION: u64 = 3;

/// The current engine recording shape paired with [`SCHEMA_VERSION`].
pub const TRACE_SCHEMA: &str = "rust-mutants-trace-v1";

/// The domain separator the engine's identities are hashed under.
pub const ID_DOMAIN: &str = "rust-mutants-id-v1";

/// How many characters of an identity a person types.
pub const DISPLAY_ID_LENGTH: usize = 20;

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const STEP_LIMIT_REACHED: &str = "step_limit_reached";
const WAITED: &str = "waited";
const INCONCLUSIVE: &str = "inconclusive";
const ERRORED: &str = "errored";
const NOT_RUN: &str = "not_run";
const UNREACHED: &str = "unreached";
const DISCHARGED: &str = "discharged";
const UNSELECTED: &str = "unselected";
const STOPPED_EARLY: &str = "stopped-early";
const BRANCH_NEVER_TAKEN: &str = "branch-never-taken";
const NEVER_INFECTED: &str = "never-infected";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const STEP_LIMIT_REACHED_MUTANT: &str = "step-limit-reached-mutant";
const WAITED_MUTANT: &str = "waited-mutant";
const UNREACHED_MUTANT: &str = "unreached-mutant";
const DISCHARGED_MUTANT: &str = "discharged-mutant";
const INCONCLUSIVE_MUTANT: &str = "inconclusive-mutant";
const ERRORED_MUTANT: &str = "errored-mutant";
const NOT_RUN_MUTANT: &str = "not-run-mutant";
const STALE_EXPECTATION: &str = "stale-expectation";
const UNMATCHED_EXPECTATION: &str = "unmatched-expectation";
/// Every standing a claim can have, as a report writes it.
pub const CLAIM_STANDINGS: [&str; 4] = ["met", "stale", "unmatched", "unjudged"];
const MET: &str = CLAIM_STANDINGS[0];
const STALE: &str = CLAIM_STANDINGS[1];
const UNMATCHED: &str = CLAIM_STANDINGS[2];
const UNJUDGED: &str = CLAIM_STANDINGS[3];

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
    /// A JSON evidence document supplied to the audit is corrupt.
    #[error("{path}: not an evidence document this audit can read: {source}")]
    MalformedEvidence {
        /// The evidence document.
        path: String,
        /// What the JSON reader found there.
        #[source]
        source: serde_json::Error,
    },
    /// An explicitly supplied recording contains a corrupt event.
    #[error("{path}: not a recording this audit can read: {source}")]
    MalformedRecording {
        /// The recording.
        path: String,
        /// Which line failed to parse.
        #[source]
        source: crate::route::ReadError,
    },
    /// A recording declares another trace contract.
    #[error("{path}: trace schema {schema:?} is not current schema {TRACE_SCHEMA:?}")]
    UnsupportedTrace {
        /// The recording.
        path: String,
        /// The schema declared by its run-start event.
        schema: String,
    },
    /// An explicitly supplied acceptance ledger is not TOML.
    #[error("{path}: not a ledger this audit can read: {source}")]
    MalformedLedger {
        /// The ledger.
        path: String,
        /// What the TOML reader found there.
        #[source]
        source: toml::de::Error,
    },
    /// The document is JSON and calls itself something other than a run report.
    #[error("{path}: {document_type:?} is not the run report this audit re-decides")]
    Unrecognised {
        /// The document.
        path: String,
        /// What it calls itself.
        document_type: String,
    },
    /// The document is a run report, but not the current report shape this audit implements.
    #[error("{path}: run-report schema {schema_version:?} is not current schema {SCHEMA_VERSION}")]
    UnsupportedVersion {
        /// The document.
        path: String,
        /// The version it declares, absent when it declares none.
        schema_version: Option<u64>,
    },
}

impl crate::error::Coded for AuditError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Unreadable { .. } => crate::error::XtCode::EngineUnreadable,
            Self::Unparsable { .. } => crate::error::XtCode::EngineUnparsable,
            Self::MalformedEvidence { .. } => crate::error::XtCode::EngineEvidence,
            Self::MalformedRecording { .. } | Self::UnsupportedTrace { .. } => {
                crate::error::XtCode::EngineRecording
            }
            Self::MalformedLedger { .. } => crate::error::XtCode::EngineLedger,
            Self::Unrecognised { .. } | Self::UnsupportedVersion { .. } => {
                crate::error::XtCode::EngineUnrecognised
            }
        }
    }
}

/// What the re-decision was able to conclude about one thing it looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
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
    /// Every reached site and every kill, against the items the entry markers say each test entered.
    Entry,
}

impl Layer {
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
            Self::Entry => "entry",
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
    #[cfg(feature = "testkit")]
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

    /// The exit code this audit earns.
    /// A run that could not be read at all never reaches here and earns [`EXIT_UNREADABLE`] instead.
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

/// One named evidence document supplied to the audit.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// Where the document was read from.
    pub path: &'a str,
    /// The document's bytes after UTF-8 filesystem decoding.
    pub text: &'a str,
}

/// What one run was audited against beyond its own report.
#[derive(Debug, Default)]
pub struct Evidence<'a> {
    /// The recording the run kept, when it kept one.
    pub recorded: Option<Source<'a>>,
    /// The reports of the other parts of this catalog, when the run was one part.
    pub shards: Vec<Source<'a>>,
    /// The ledger of accepted survivors, as the configuration file holds it.
    pub ledger: Option<Source<'a>>,
    /// Whether the census of the walk's own decisions is re-derived.
    pub sites: bool,
    /// What the coverage layer measured, as the run kept it.
    pub reached: Option<Source<'a>>,
    /// The catalog the run kept, which holds the body each branch proof names.
    pub catalog: Option<Source<'a>>,
    /// The names of the probe logs the run kept.
    pub probe_logs: Vec<String>,
    /// What the guards recorded, as the run kept it.
    pub touched: Option<Source<'a>>,
}

/// Evidence after every serialization boundary has been crossed without loss.
struct CheckedEvidence<'a> {
    recorded: Option<CheckedRecording>,
    shards: Vec<(&'a str, Report)>,
    ledger: Option<toml::Table>,
    sites: bool,
    reached: Option<Value>,
    catalog: Option<Value>,
    probe_logs: &'a [String],
    touched: Option<Value>,
}

/// One recording whose every non-empty line is JSON.
struct CheckedRecording {
    events: Vec<Value>,
    routing: crate::route::Routing,
}

impl<'a> Evidence<'a> {
    fn check(&'a self) -> Result<CheckedEvidence<'a>, AuditError> {
        let recorded = self
            .recorded
            .map(|source| {
                let events = crate::route::events(source.text).map_err(|error| {
                    AuditError::MalformedRecording {
                        path: source.path.to_owned(),
                        source: error,
                    }
                })?;
                if let Some(schema) = events.iter().find_map(|event| {
                    (string(event, "type").as_deref() == Some("run-start"))
                        .then(|| string(event, "schema"))
                        .and_then(std::convert::identity)
                }) && schema != TRACE_SCHEMA
                {
                    return Err(AuditError::UnsupportedTrace {
                        path: source.path.to_owned(),
                        schema,
                    });
                }
                let routing = crate::route::from_events(&events);
                Ok(CheckedRecording { events, routing })
            })
            .transpose()?;
        let shards = self
            .shards
            .iter()
            .map(|source| parse_report_evidence(*source).map(|report| (source.path, report)))
            .collect::<Result<Vec<_>, _>>()?;
        let ledger =
            self.ledger
                .map(|source| {
                    source.text.parse::<toml::Table>().map_err(|error| {
                        AuditError::MalformedLedger {
                            path: source.path.to_owned(),
                            source: error,
                        }
                    })
                })
                .transpose()?;
        Ok(CheckedEvidence {
            recorded,
            shards,
            ledger,
            sites: self.sites,
            reached: self
                .reached
                .map(|source| parse_typed_evidence(source, wire::validate_reached))
                .transpose()?,
            catalog: self.catalog.map(parse_evidence).transpose()?,
            probe_logs: &self.probe_logs,
            touched: self
                .touched
                .map(|source| parse_typed_evidence(source, wire::validate_touched))
                .transpose()?,
        })
    }
}

fn parse_report_evidence(source: Source<'_>) -> Result<Report, AuditError> {
    let document = wire::decode(source.text).map_err(|error| AuditError::MalformedEvidence {
        path: source.path.to_owned(),
        source: error,
    })?;
    if document.document_type() != DOCUMENT_TYPE {
        return Err(AuditError::Unrecognised {
            path: source.path.to_owned(),
            document_type: document.document_type().to_owned(),
        });
    }
    if document.schema_version() != SCHEMA_VERSION {
        return Err(AuditError::UnsupportedVersion {
            path: source.path.to_owned(),
            schema_version: Some(document.schema_version()),
        });
    }
    Ok(document.into_report())
}

fn parse_evidence(source: Source<'_>) -> Result<Value, AuditError> {
    crate::strictjson::from_str(source.text).map_err(|error| AuditError::MalformedEvidence {
        path: source.path.to_owned(),
        source: error,
    })
}

fn parse_typed_evidence(
    source: Source<'_>,
    validate: fn(&Value) -> Result<(), serde_json::Error>,
) -> Result<Value, AuditError> {
    let value = parse_evidence(source)?;
    validate(&value).map_err(|error| AuditError::MalformedEvidence {
        path: source.path.to_owned(),
        source: error,
    })?;
    Ok(value)
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
/// [`AuditError::Unparsable`] for a document that is not JSON, and [`AuditError::Unrecognised`] for one that is not a run report.
pub fn audit(path: &str, text: &str, evidence: &Evidence<'_>) -> Result<Audit, AuditError> {
    let document = wire::decode(text).map_err(|source| AuditError::Unparsable {
        path: path.to_owned(),
        source,
    })?;
    let document_type = document.document_type().to_owned();
    if document_type != DOCUMENT_TYPE {
        return Err(AuditError::Unrecognised {
            path: path.to_owned(),
            document_type,
        });
    }
    let schema_version = document.schema_version();
    if schema_version != SCHEMA_VERSION {
        return Err(AuditError::UnsupportedVersion {
            path: path.to_owned(),
            schema_version: Some(schema_version),
        });
    }
    let report = document.into_report();
    let evidence = evidence.check()?;
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
    merge(&report, &evidence, &mut audit);
    proofs(&report, &evidence, &mut audit);
    sites(&evidence, &mut audit);
    trace(&report, evidence.recorded.as_ref(), &mut audit);
    ledger(&report, evidence.ledger.as_ref(), &mut audit);
    work(&report, evidence.recorded.as_ref(), &mut audit);
    touch(&report, evidence.touched.as_ref(), &mut audit);
    entry(&report, evidence.touched.as_ref(), &mut audit);
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// One mutant row, as a reader sees it.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is an independent fact the published report row states"
)]
struct Row {
    index: u64,
    route: Option<RouteDecision>,
    id: String,
    display_id: String,
    path: String,
    rule: String,
    rule_version: u64,
    start_byte: u64,
    end_byte: u64,
    source_digest: String,
    original: String,
    replacement: String,
    outcome: Outcome,
    step_notice: Option<StepNotice>,
    target: String,
    tests_run: Option<u64>,
    killed_by: Vec<String>,
    item: String,
    retried: bool,
    lingered: bool,
    expected: bool,
    unreached: bool,
    not_run_reason: Option<NotRunReason>,
    source_run_id: Option<String>,
}

/// The independently read fields of a verified runtime step notice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StepNotice {
    nonce: String,
    catalog: String,
    mutant: String,
    limit: u64,
    observed: u64,
}

/// One of the only outcomes the report contract can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Killed,
    Survived,
    StepLimitReached,
    Waited,
    Inconclusive,
    Errored,
    NotRun,
}

impl Outcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Killed => KILLED,
            Self::Survived => SURVIVED,
            Self::StepLimitReached => STEP_LIMIT_REACHED,
            Self::Waited => WAITED,
            Self::Inconclusive => INCONCLUSIVE,
            Self::Errored => ERRORED,
            Self::NotRun => NOT_RUN,
        }
    }
}

impl PartialEq<&str> for Outcome {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a row deliberately has no execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum NotRunReason {
    Unreached,
    Discharged,
    Interrupted,
    Unselected,
    StoppedEarly,
}

impl NotRunReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unreached => UNREACHED,
            Self::Discharged => DISCHARGED,
            Self::Interrupted => "interrupted",
            Self::Unselected => UNSELECTED,
            Self::StoppedEarly => STOPPED_EARLY,
        }
    }
}

impl PartialEq<&str> for NotRunReason {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for NotRunReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A complete routing decision.
/// Its absence is represented once, by the enclosing `Option`, rather than by five mutually inconsistent sentinels.
#[derive(Debug, Clone)]
struct RouteDecision {
    granularity: Granularity,
    fallback: Option<String>,
    tests: BTreeMap<String, BTreeSet<String>>,
    reaching: Vec<String>,
    executed: Vec<String>,
    discharged: Vec<(String, String)>,
}

/// The only granularities the engine report contract permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Granularity {
    All,
    Block,
    Test,
    Discharged,
    Unreached,
}

impl Granularity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Block => "block",
            Self::Test => "test",
            Self::Discharged => DISCHARGED,
            Self::Unreached => UNREACHED,
        }
    }
}

impl fmt::Display for Granularity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
        self.not_run_reason.is_some_and(|held| held == reason)
    }

    const fn complete(&self) -> bool {
        !self.source_digest.is_empty() && !self.path.is_empty() && !self.rule.is_empty()
    }
}

/// One refused candidate, as a reader sees it.
#[derive(Debug, Clone)]
struct Refusal {
    index: u64,
    display_id: String,
}

/// One claim a reviewer declared, as the run left it.
#[derive(Debug, Clone)]
struct Claim {
    id: String,
    mutant: Option<String>,
    standing: ClaimStanding,
}

/// One thing that stops the run from being clean.
#[derive(Debug, Clone)]
struct Finding {
    kind: FindingKind,
    mutant: Option<String>,
}

/// The closed result of resolving an expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClaimStanding {
    Met,
    Stale,
    Unmatched,
    Unjudged,
}

impl ClaimStanding {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Met => MET,
            Self::Stale => STALE,
            Self::Unmatched => UNMATCHED,
            Self::Unjudged => UNJUDGED,
        }
    }
}

impl PartialEq<&str> for ClaimStanding {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// The complete set of findings the v1 report contract permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FindingKind {
    SurvivingMutant,
    StepLimitReachedMutant,
    WaitedMutant,
    InconclusiveMutant,
    ErroredMutant,
    NotRunMutant,
    UnreachedMutant,
    DischargedMutant,
    StaleExpectation,
    UnmatchedExpectation,
    UnmatchedSkip,
}

impl FindingKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SurvivingMutant => SURVIVING_MUTANT,
            Self::StepLimitReachedMutant => STEP_LIMIT_REACHED_MUTANT,
            Self::WaitedMutant => WAITED_MUTANT,
            Self::InconclusiveMutant => INCONCLUSIVE_MUTANT,
            Self::ErroredMutant => ERRORED_MUTANT,
            Self::NotRunMutant => NOT_RUN_MUTANT,
            Self::UnreachedMutant => UNREACHED_MUTANT,
            Self::DischargedMutant => DISCHARGED_MUTANT,
            Self::StaleExpectation => STALE_EXPECTATION,
            Self::UnmatchedExpectation => UNMATCHED_EXPECTATION,
            Self::UnmatchedSkip => "unmatched-skip",
        }
    }
}

impl PartialEq<&str> for FindingKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for FindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The report, read as data.
#[derive(Debug)]
struct Report {
    run_id: String,
    targets: Vec<String>,
    tool_version: String,
    workspace_digest: String,
    catalog_digest: String,
    selection: wire::Selection,
    mutant_steps: Option<u64>,
    interrupted: bool,
    exit_code: u8,
    columns: BTreeMap<String, u64>,
    score: Option<(u64, u64, f64)>,
    mutants: Vec<Row>,
    rejections: Vec<Refusal>,
    expectations: Vec<Claim>,
    findings: Vec<Finding>,
    skip_counts: Vec<u64>,
}

impl Report {
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

/// A count widened without a fallible or truncating conversion.
/// Rust supports only pointer widths that fit the report's `u64` count contract; an unknown future width fails this crate at compile time instead of inventing a value.
#[cfg(target_pointer_width = "64")]
const fn count(value: usize) -> u64 {
    u64::from_be_bytes(value.to_be_bytes())
}

#[cfg(target_pointer_width = "32")]
const fn count(value: usize) -> u64 {
    u64::from(u32::from_be_bytes(value.to_be_bytes()))
}

#[cfg(target_pointer_width = "16")]
const fn count(value: usize) -> u64 {
    u64::from(u16::from_be_bytes(value.to_be_bytes()))
}

#[cfg(not(any(
    target_pointer_width = "16",
    target_pointer_width = "32",
    target_pointer_width = "64"
)))]
compile_error!("engine audit counts require a pointer width no wider than u64");

/// `n thing` or `n things`.
fn plural(count: usize, thing: &str) -> String {
    if count == 1 {
        format!("{count} {thing}")
    } else {
        format!("{count} {thing}s")
    }
}

#[cfg(test)]
mod tests {
    use super::{CLAIM_STANDINGS, ClaimStanding};

    #[test]
    fn every_standing_the_audit_decodes_is_one_it_names() {
        let every = [
            ClaimStanding::Met,
            ClaimStanding::Stale,
            ClaimStanding::Unmatched,
            ClaimStanding::Unjudged,
        ];
        for standing in every {
            match standing {
                ClaimStanding::Met
                | ClaimStanding::Stale
                | ClaimStanding::Unmatched
                | ClaimStanding::Unjudged => {}
            }
            assert!(
                CLAIM_STANDINGS.contains(&standing.as_str()),
                "{standing:?} decodes and is not among the standings the audit names"
            );
        }
        assert_eq!(every.len(), CLAIM_STANDINGS.len());
    }
}
