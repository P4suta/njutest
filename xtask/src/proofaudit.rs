// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed run recorded.
//!
//! [ADR 0004](../../docs/adr/0004-proof-layers-not-budgets.md) ships a proof layer only against a re-implementation that never calls the runner's, so nothing here consults the code that wrote the report: every verdict is re-derived from the recording alone, and wherever the recording does not carry enough to re-derive one, that is said plainly rather than read as agreement.

pub mod sentinel;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use sha2::Digest as _;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "njutest-assurance-report-v1.json";

/// The schema this audit knows how to re-decide.
pub const SCHEMA: &str = "njutest-assurance-report-v1";

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const UNREACHED: &str = "unreached";
const REJECTED: &str = "compile-rejected";
const STEP_LIMIT_REACHED: &str = "step-limit-reached";
const WAITED: &str = "waited";
const UNCONFIRMED: &str = "unconfirmed";
const ERRORED: &str = "errored";
const PASSED: &str = "passed";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNNOTICED_FAULT: &str = "unnoticed-fault";
const BROKEN_UNDER_FAULT: &str = "broken-under-fault";
const NOT_MEASURED_FINDING: &str = "not-measured";
/// Every finding kind that is something wrong with the code under test, as `docs/report-v1.md` marks them, which is what lets a run conclude DEFECT.
pub const DEFECT_KINDS: [&str; 4] = [
    "build-failure",
    "failing-test",
    "undefined-behaviour",
    "broken-under-fault",
];
const WAITED_MUTANT: &str = "waited-mutant";
const STEP_LIMIT_REACHED_MUTANT: &str = "step-limit-reached-mutant";
const FAILING_TEST: &str = "failing-test";
const TARGET_MISSING: &str = "target-missing";
const UNMATCHED_ACCEPTANCE: &str = "unmatched-acceptance";
const EVERYTHING: &str = "all";
const EQUIVALENT: &str = "equivalent";

/// Why a recording could not be re-decided at all.
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
    /// The explicitly supplied recording contains a corrupt event.
    #[error("{path}: not a recording this audit can read: {source}")]
    MalformedRecording {
        /// The recording.
        path: String,
        /// Which line failed to parse.
        #[source]
        source: crate::route::ReadError,
    },
    /// The document is a whole report of a shape this audit does not project onto the one build it re-decides.
    #[error("{path}: {shape}; this audit re-decides one configured build measured whole")]
    Unprojected {
        /// The document.
        path: String,
        /// What it holds instead.
        shape: Unprojectable,
    },
    /// The document is JSON and calls itself something other than the assurance report.
    #[error("{path}: {schema:?} is not the assurance report this audit re-decides")]
    Unrecognised {
        /// The document.
        path: String,
        /// What it calls itself.
        schema: String,
    },
}

/// What a report holds instead of the one configured build measured whole this audit re-decides.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unprojectable {
    /// The document is one part of a catalog rather than a complete answer.
    #[error("a {kind} document is one part of a catalog")]
    Part {
        /// What the document calls itself.
        kind: String,
    },
    /// The report measured more than one configured build, or none.
    #[error("a report of {count} configured builds")]
    Builds {
        /// How many it holds.
        count: usize,
    },
    /// The build was measured in parts, or its part is missing.
    #[error("a build of {count} parts")]
    Parts {
        /// How many it holds.
        count: usize,
    },
}

/// What the re-decision was able to conclude about one thing it looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    /// The recording contradicts itself, or rests a verdict on evidence it does not hold.
    Violated,
    /// The recording does not carry what a re-decision would need, which is neither a pass nor a failure.
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

/// The part of a recording one re-decision was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Layer {
    /// The columns of the accounting, against the records they summarise and against the verdict they carry.
    Accounting,
    /// The target a kill is attributed to.
    Killers,
    /// The correspondence between the mutations nothing noticed and the findings that name them.
    Findings,
    /// Whether an unmatched acceptance really fails to resolve in the complete catalog.
    Acceptances,
    /// The earlier run a disposition was read back from.
    Reuse,
    /// The layers that removed an execution, held to the kills the run recorded.
    Proofs,
    /// The targets the recording says were put to mutations and noticed none, held to the findings that name them.
    Hollow,
    /// The faults a seam recording licensed, re-derived and held to what the run says it put and what nothing noticed.
    Wire,
    /// Affirmative model answers re-derived from retained generated source and raw Kani JSON.
    Model,
    /// Which targets reached something different on a control than on their baseline, re-derived from the engine's touch records and held to what the report says of each.
    Drift,
    /// What each fault site came to, re-derived from the fault executions alone and held to the report's records, counts and findings.
    Faults,
}

impl Layer {
    /// What to write in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Accounting => "accounting",
            Self::Killers => "killers",
            Self::Findings => "findings",
            Self::Acceptances => "acceptances",
            Self::Reuse => "reuse",
            Self::Proofs => "proofs",
            Self::Hollow => "hollow",
            Self::Wire => "wire",
            Self::Model => "model",
            Self::Drift => "drift",
            Self::Faults => "faults",
        }
    }
}

/// One thing the re-decision has to say about one part of one recording.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Remark {
    /// The part of the recording it is about.
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

/// What an independent re-decision made of one run's recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// The run the recording names.
    pub run_id: String,
    /// How many mutant records it re-decided over.
    pub mutants: usize,
    /// How many target records it re-decided over.
    pub targets: usize,
    /// Everything it has to say, grouped by layer with the violations of each first.
    pub remarks: Vec<Remark>,
}

impl Audit {
    /// How many things the recording does not support.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.standing(Standing::Violated)
    }

    /// How many things the recording does not carry enough to re-decide.
    #[must_use]
    pub fn unaudited(&self) -> usize {
        self.standing(Standing::Unaudited)
    }

    /// Whether one layer found something the run does not support.
    #[must_use]
    pub fn violated(&self, layer: Layer) -> bool {
        self.remarks
            .iter()
            .any(|remark| remark.layer == layer && remark.standing == Standing::Violated)
    }

    /// The exit code this audit earns.
    /// A recording that could not be read at all never reaches here and earns [`EXIT_UNREADABLE`] instead.
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
            "proofaudit: {}: {} and {} re-decided; {}, {} unaudited",
            self.run_id,
            plural(self.mutants, "mutant"),
            plural(self.targets, "target"),
            plural(self.violations(), "violation"),
            self.unaudited()
        )
    }
}

/// Where one layer's re-decisions are written down, so that a check names only the subject and the sentence and never repeats which layer it belongs to.
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

    /// One column against the records it summarises.
    fn tally(&mut self, column: Column<'_>) {
        let Column {
            subject,
            recorded,
            derived,
            records,
        } = column;
        match (recorded, derived) {
            (None, _) => self.unaudited(
                subject,
                format!("the recording omits this column, so {records} answer to nothing"),
            ),
            (Some(_), None) => self.violated(
                subject,
                format!("{records} exceed the u64 count the report wire can represent"),
            ),
            (Some(count), Some(derived)) if count != derived => self.violated(
                subject,
                format!(
                    "the column says {count} and {records} come to {derived}; a report that \
                     contradicts itself is not evidence of anything"
                ),
            ),
            (Some(_), Some(_)) => {}
        }
    }

    /// One equation the assurance contract states, over the columns alone.
    fn holds(&mut self, equation: Equation<'_>) {
        let Equation {
            subject,
            relation,
            sides,
            because,
        } = equation;
        let (Some(left), Some(right)) = sides else {
            self.unaudited(
                subject,
                format!(
                    "the recording omits a column this equation is over, so it cannot be \
                     re-decided: {because}"
                ),
            );
            return;
        };
        if !relation.holds(left, right) {
            self.violated(
                subject,
                format!(
                    "the recording's own columns come to {left} where they must come to {} \
                     {right}; {because}",
                    relation.word()
                ),
            );
        }
    }
}

/// One column of the accounting and the records that must agree with it.
#[derive(Debug, Clone, Copy)]
struct Column<'a> {
    subject: &'a str,
    recorded: Option<u64>,
    derived: Option<u64>,
    records: &'a str,
}

/// One relation the columns must stand in to each other, and why.
#[derive(Debug, Clone, Copy)]
struct Equation<'a> {
    subject: &'a str,
    relation: Relation,
    sides: (Option<u64>, Option<u64>),
    because: &'a str,
}

/// What a run recorded beside its report: the runner's recording and every engine recording under it, each as its path and its text.
#[derive(Debug, Clone, Copy)]
pub struct Recorded<'a> {
    /// The runner's recording, when the run kept one.
    pub runner: Option<(&'a str, &'a str)>,
    /// Every configured build's engine recording, in namespace order.
    pub engines: &'a [(String, String)],
}

/// Re-decides a report against what the run recorded beside it and, when `run` is present, re-reads every retained model-checker artifact from that exact run directory.
///
/// # Errors
/// [`AuditError::Unparsable`] for a document that is not JSON, [`AuditError::Unprojected`] for a report this audit does not re-decide as one build,
/// [`AuditError::Unrecognised`] for one that is not the assurance report, [`AuditError::MalformedRecording`] for a recording with a corrupt line, and the corresponding retained-artifact error when `run` cannot be re-read.
pub fn audit_with(
    path: &str,
    text: &str,
    recorded: Recorded<'_>,
    run: Option<&Path>,
) -> Result<Audit, AuditError> {
    let read: serde_json::Value =
        crate::strictjson::from_str(text).map_err(|source| AuditError::Unparsable {
            path: path.to_owned(),
            source,
        })?;
    let document = projected(read).map_err(|shape| AuditError::Unprojected {
        path: path.to_owned(),
        shape,
    })?;
    let schema = document
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if schema != SCHEMA {
        return Err(AuditError::Unrecognised {
            path: path.to_owned(),
            schema: schema.to_owned(),
        });
    }
    let recorded_runner = recorded.runner;
    let recording = Recording::of(&document);
    let routing = recorded_runner
        .map(|(recording_path, text)| {
            crate::route::read(text).map_err(|source| AuditError::MalformedRecording {
                path: recording_path.to_owned(),
                source,
            })
        })
        .transpose()?;
    let watched = recorded_runner
        .map(|(recording_path, text)| {
            crate::wire::read(text).map_err(|source| AuditError::MalformedRecording {
                path: recording_path.to_owned(),
                source,
            })
        })
        .transpose()?;
    let faulted = recorded_runner
        .map(|(recording_path, text)| {
            crate::faults::read(text).map_err(|source| AuditError::MalformedRecording {
                path: recording_path.to_owned(),
                source,
            })
        })
        .transpose()?;
    let engines = recorded
        .engines
        .iter()
        .map(|(recording_path, text)| {
            crate::drift::read(text).map_err(|source| AuditError::MalformedRecording {
                path: recording_path.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut audit = Audit {
        run_id: recording.run_id.clone(),
        mutants: recording.mutants.len(),
        targets: recording.targets.len(),
        remarks: Vec::new(),
    };
    target_columns(&recording, &mut audit);
    mutant_columns(&recording, &mut audit);
    equations(&recording, &mut audit);
    verdict(&recording, &mut audit);
    killers(&recording, &mut audit);
    findings(&recording, &mut audit);
    acceptances(&recording, &mut audit);
    reuse(&recording, &mut audit);
    proofs(&recording, routing.as_ref(), &mut audit);
    hollow(&recording, routing.as_ref(), &mut audit);
    wire(&recording, watched.as_ref(), &mut audit);
    models(&recording, run, &mut audit);
    drift(&recording, &engines, &mut audit);
    faults(&recording, faulted.as_ref(), &mut audit);
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// The flat view of one configured build measured whole that every layer re-decides, taken from a complete report or as it is.
fn projected(document: serde_json::Value) -> Result<serde_json::Value, Unprojectable> {
    let Some(kind) = document.get("document_type") else {
        return Ok(document);
    };
    if kind.as_str() != Some("complete") {
        return Err(Unprojectable::Part {
            kind: kind.to_string(),
        });
    }
    let report = document.get("report").cloned().unwrap_or_default();
    let builds = rows(&report, "builds");
    let [build] = builds else {
        return Err(Unprojectable::Builds {
            count: builds.len(),
        });
    };
    let parts = rows(build, "parts");
    let [part] = parts else {
        return Err(Unprojectable::Parts { count: parts.len() });
    };
    let mut flat = serde_json::Map::new();
    for key in [
        "schema",
        "schema_version",
        "run_id",
        "run_kind",
        "contract",
        "scope",
    ] {
        if let Some(value) = report.get(key) {
            flat.insert(key.to_owned(), value.clone());
        }
    }
    for key in [
        "toolchain",
        "accounting",
        "targets",
        "mutants",
        "limitations",
        "drift",
        "faults",
    ] {
        if let Some(value) = part.get(key) {
            flat.insert(key.to_owned(), value.clone());
        }
    }
    let findings: Vec<serde_json::Value> = rows(&report, "global_findings")
        .iter()
        .chain(rows(part, "findings"))
        .cloned()
        .collect();
    flat.insert("findings".to_owned(), serde_json::Value::Array(findings));
    let models = report
        .get("model_completion")
        .and_then(|completion| completion.get("batch"))
        .map(|batch| rows(batch, "records").to_vec())
        .unwrap_or_default();
    flat.insert("models".to_owned(), serde_json::Value::Array(models));
    Ok(serde_json::Value::Object(flat))
}

/// The finding a report raises about a target whose baseline reach moved.
const UNSTABLE_BASELINE: &str = "unstable-baseline";

/// The limitation a report states about the targets no comparable control measured.
const DRIFT_NOT_MEASURED: &str = "drift-not-measured";

/// Which targets moved between their baseline and a control, re-derived from the engine's touch records and held to the report's records, findings, and limitation.
fn drift(recording: &Recording<'_>, engines: &[crate::drift::Touched], audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Drift);
    let recorded = recording.document.get("drift").map(|rows| {
        rows.as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|row| {
                (
                    field(row, "target").unwrap_or_default(),
                    field(row, "state").unwrap_or_default(),
                )
            })
            .collect::<Vec<(String, String)>>()
    });
    let touched = match (engines, recorded.as_ref()) {
        ([], None) => return,
        ([], Some(_)) => {
            notes.unaudited(
                "drift",
                "the run kept no engine recording, so which targets moved between their \
                 baseline and a control cannot be re-derived"
                    .to_owned(),
            );
            return;
        }
        ([one], _) => one,
        (several, _) => {
            notes.unaudited(
                "drift",
                format!(
                    "the recording holds {} engine recordings and the report is one build's, \
                     so which of them it answers to cannot be told from the recording",
                    several.len()
                ),
            );
            return;
        }
    };
    if touched.unreadable > 0 {
        notes.unaudited(
            "drift",
            format!(
                "{} touch record(s) do not say which run they were measured on or what it \
                 reached, so what they would have shown cannot be counted as agreement",
                touched.unreadable
            ),
        );
    }
    let derived = crate::drift::standings(touched);
    let Some(recorded) = recorded else {
        if !derived.is_empty() {
            notes.violated(
                "drift",
                format!(
                    "the engine measured the baseline reach of {} target(s) and the report \
                     records nothing about whether any of it held",
                    derived.len()
                ),
            );
        }
        return;
    };
    held_to_records(&derived, &recorded, &mut notes);
    if recording.shard.is_some() {
        return;
    }
    held_to_findings(recording, &derived, &mut notes);
    held_to_limitation(recording, &derived, &mut notes);
}

fn held_to_records(
    derived: &BTreeMap<String, crate::drift::Standing>,
    recorded: &[(String, String)],
    notes: &mut Notes<'_>,
) {
    for (target, standing) in derived {
        let said: Vec<&str> = recorded
            .iter()
            .filter(|(named, _)| named == target)
            .map(|(_, state)| state.as_str())
            .collect();
        match said.as_slice() {
            [one] if *one == standing.name() => {}
            [one] => notes.violated(
                target,
                format!(
                    "the engine's touch records say {target} {} and the report records it as \
                     {one}",
                    standing.name()
                ),
            ),
            others => notes.violated(
                target,
                format!(
                    "the engine measured the baseline reach of {target} and the report records \
                     {} drift record(s) about it where it owes exactly one",
                    others.len()
                ),
            ),
        }
    }
    for (target, state) in recorded {
        if !derived.contains_key(target) {
            notes.violated(
                target,
                format!(
                    "the report records {target} as {state} and the engine recorded no baseline \
                     reach for it to have held or moved from"
                ),
            );
        }
        if crate::drift::Standing::parse(state).is_none() {
            notes.violated(
                target,
                format!("{state:?} is not a standing a drift record can have"),
            );
        }
    }
}

fn held_to_findings(
    recording: &Recording<'_>,
    derived: &BTreeMap<String, crate::drift::Standing>,
    notes: &mut Notes<'_>,
) {
    let owed: BTreeSet<&str> = derived
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| target.as_str())
        .collect();
    let named: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|finding| finding.kind == UNSTABLE_BASELINE)
        .map(|finding| finding.subject.as_str())
        .collect();
    for target in owed.difference(&named) {
        notes.violated(
            target,
            format!(
                "a control of {target} that passed the same tests reached something its \
                 baseline did not, and the report raises no {UNSTABLE_BASELINE} finding about it"
            ),
        );
    }
    for target in named.difference(&owed) {
        notes.violated(
            target,
            format!(
                "the report raises {UNSTABLE_BASELINE} about {target}, and the engine's touch \
                 records do not show its reach moving"
            ),
        );
    }
}

fn held_to_limitation(
    recording: &Recording<'_>,
    derived: &BTreeMap<String, crate::drift::Standing>,
    notes: &mut Notes<'_>,
) {
    let owed: Vec<&str> = derived
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::NotMeasured)
        .map(|(target, _)| target.as_str())
        .collect();
    let stated: Vec<String> = rows(recording.document, "limitations")
        .iter()
        .filter(|row| field(row, "name").as_deref() == Some(DRIFT_NOT_MEASURED))
        .map(|row| field(row, "detail").unwrap_or_default())
        .collect();
    match (owed.as_slice(), stated.as_slice()) {
        ([], []) => {}
        ([], _) => notes.violated(
            DRIFT_NOT_MEASURED,
            "the report says a target's drift was not measured, and a comparable control \
             recorded every target the baseline measured"
                .to_owned(),
        ),
        (_, []) => notes.violated(
            DRIFT_NOT_MEASURED,
            format!(
                "no comparable control recorded what {} reached, and the report does not say \
                 so",
                owed.join(", ")
            ),
        ),
        (_, [detail]) => {
            for target in &owed {
                if !listed(detail, target) {
                    notes.violated(
                        target,
                        format!(
                            "no comparable control recorded what {target} reached, and the \
                             {DRIFT_NOT_MEASURED} limitation does not name it"
                        ),
                    );
                }
            }
        }
        (_, several) => notes.violated(
            DRIFT_NOT_MEASURED,
            format!(
                "the report states {} {DRIFT_NOT_MEASURED} limitations where one names every \
                 target",
                several.len()
            ),
        ),
    }
}

/// Whether a limitation's detail names `target` in its closing list, which is how a report names the targets a limitation is about.
fn listed(detail: &str, target: &str) -> bool {
    detail
        .rsplit_once(" (")
        .and_then(|(_, list)| list.strip_suffix(')'))
        .is_some_and(|list| list.split(", ").any(|one| one == target))
}

#[derive(Debug)]
struct TargetRow {
    id: String,
    name: String,
    status: String,
}

#[derive(Debug)]
struct MutantRow {
    id: String,
    display_id: String,
    outcome: String,
    acceptance: AcceptanceFact,
    killed_by: Option<String>,
    reused: bool,
    source_run_id: Option<String>,
}

/// The three distinct facts a report can state about row-local review acceptance.
///
/// Keeping the missing case as its own variant prevents an absent field from being confused with an explicit rejection while the independent audit is re-deriving answerability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcceptanceFact {
    Missing,
    Rejected,
    Accepted,
}

impl AcceptanceFact {
    fn from_json(value: Option<&serde_json::Value>) -> Self {
        match value.and_then(serde_json::Value::as_bool) {
            Some(false) => Self::Rejected,
            Some(true) => Self::Accepted,
            None => Self::Missing,
        }
    }
}

impl MutantRow {
    fn label(&self) -> &str {
        if !self.display_id.is_empty() {
            &self.display_id
        } else if self.id.is_empty() {
            "a mutant the recording does not name"
        } else {
            &self.id
        }
    }

    fn answers_to(&self, subject: &str) -> bool {
        subject == self.display_id || subject == self.id
    }
}

#[derive(Debug)]
struct FindingRow {
    kind: String,
    subject: String,
}

#[derive(Debug)]
struct ModelRow {
    mutant: String,
    decision: String,
    evidence: Option<serde_json::Value>,
    attempt: Option<serde_json::Value>,
    answer: serde_json::Value,
    raw: serde_json::Value,
}

fn models(recording: &Recording<'_>, run: Option<&Path>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Model);
    let mut identities = BTreeSet::new();
    if recording.contract != "verified-v1" && !recording.models.is_empty() {
        notes.violated(
            "models",
            format!(
                "contract {:?} cannot carry verified-v1 model records",
                recording.contract
            ),
        );
    }
    for model in &recording.models {
        audit_model(recording, run, model, (&mut identities, &mut notes));
    }
    if recording.contract == "verified-v1" {
        for mutant in &recording.mutants {
            if matches!(
                mutant.outcome.as_str(),
                SURVIVED | "model-noticed" | "model-proved"
            ) && !identities.contains(mutant.id.as_str())
            {
                notes.violated(
                    mutant.label(),
                    "a verified-v1 test survivor has no exactly corresponding model record"
                        .to_owned(),
                );
            }
        }
    }
    model_columns(recording, &mut notes);
}

fn audit_model<'model>(
    recording: &Recording<'_>,
    run: Option<&Path>,
    model: &'model ModelRow,
    state: (&mut BTreeSet<&'model str>, &mut Notes<'_>),
) {
    let (identities, notes) = state;
    if !model_shape(model, &recording.target) {
        notes.violated(
            &model.mutant,
            "the model record is not the exact closed shape for its decision".to_owned(),
        );
        return;
    }
    if model.mutant.is_empty() || !identities.insert(model.mutant.as_str()) {
        notes.violated(
            &model.mutant,
            "model records must carry unique, non-empty full mutation identities".to_owned(),
        );
        return;
    }
    let context = ModelAuditContext { recording, run };
    match model.decision.as_str() {
        "noticed" => audit_affirmative(
            context,
            model,
            ("model-noticed", crate::modelaudit::Answer::Noticed),
            notes,
        ),
        "proved" => audit_affirmative(
            context,
            model,
            ("model-proved", crate::modelaudit::Answer::Proved),
            notes,
        ),
        "ineligible" => audit_nonaffirmative(model, model_outcome(recording, model), notes),
        "undecided" => audit_attempt(context, model, notes),
        _ => notes.violated(
            &model.mutant,
            "the model decision is outside the closed set".to_owned(),
        ),
    }
}

#[derive(Clone, Copy)]
struct ModelAuditContext<'a, 'document> {
    recording: &'a Recording<'document>,
    run: Option<&'a Path>,
}

fn model_outcome<'a>(recording: &'a Recording<'_>, model: &ModelRow) -> Option<&'a str> {
    recording
        .mutants
        .iter()
        .find(|mutant| mutant.id == model.mutant)
        .map(|mutant| mutant.outcome.as_str())
}

fn audit_nonaffirmative(model: &ModelRow, outcome: Option<&str>, notes: &mut Notes<'_>) {
    if outcome != Some(SURVIVED) {
        notes.violated(
            &model.mutant,
            format!(
                "a non-affirmative model record must leave a test survivor as survived, not {outcome:?}"
            ),
        );
    }
}

fn audit_attempt(context: ModelAuditContext<'_, '_>, model: &ModelRow, notes: &mut Notes<'_>) {
    let outcome = model_outcome(context.recording, model);
    if outcome != Some(SURVIVED) {
        audit_nonaffirmative(model, outcome, notes);
        return;
    }
    let Some(attempt) = model.attempt.as_ref() else {
        notes.violated(
            &model.mutant,
            "an undecided model record has no closed attempt".to_owned(),
        );
        return;
    };
    let (Some(reason), Some(evidence)) = (attempt.get("reason"), attempt.get("evidence")) else {
        notes.violated(
            &model.mutant,
            "an undecided model record lacks its typed reason or attempt evidence".to_owned(),
        );
        return;
    };
    let Some(run) = context.run else {
        notes.unaudited(
            &model.mutant,
            "the caller supplied no run directory, so retained model attempt artifacts cannot be re-read"
                .to_owned(),
        );
        return;
    };
    if let Err(error) = crate::modelaudit::verify_attempt(crate::modelaudit::AttemptInput {
        run,
        report_target: &context.recording.target,
        mutant: &model.mutant,
        reason,
        evidence,
    }) {
        notes.violated(&model.mutant, error.to_string());
    }
}

fn audit_affirmative(
    context: ModelAuditContext<'_, '_>,
    model: &ModelRow,
    expected: (&str, crate::modelaudit::Answer),
    notes: &mut Notes<'_>,
) {
    let (expected_outcome, answer) = expected;
    let outcome = model_outcome(context.recording, model);
    if outcome != Some(expected_outcome) {
        notes.violated(
            &model.mutant,
            format!(
                "the model record says {} and the mutant outcome is {:?}",
                model.decision, outcome
            ),
        );
        return;
    }
    let Some(evidence) = model.evidence.as_ref() else {
        notes.violated(
            &model.mutant,
            "an affirmative model record has no evidence".to_owned(),
        );
        return;
    };
    let Some(run) = context.run else {
        notes.unaudited(
            &model.mutant,
            "the caller supplied no run directory, so retained model artifacts cannot be re-read"
                .to_owned(),
        );
        return;
    };
    let input = crate::modelaudit::Input {
        run,
        report_target: &context.recording.target,
        mutant: &model.mutant,
        answer,
        evidence,
    };
    if let Err(error) = crate::modelaudit::verify(input) {
        notes.violated(&model.mutant, error.to_string());
    }
}

fn model_shape(model: &ModelRow, report_target: &str) -> bool {
    if !exact_keys(&model.raw, &["mutant", "answer"]) {
        return false;
    }
    match model.decision.as_str() {
        "ineligible" => {
            exact_keys(&model.answer, &["decision", "reason"])
                && model
                    .answer
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|reason| {
                        matches!(
                            reason,
                            "source-encoding"
                                | "source-digest"
                                | "candidate"
                                | "identity"
                                | "source-syntax"
                                | "enclosing-function"
                                | "function-shape"
                                | "no-symbolic-input"
                                | "argument-pattern"
                                | "input-type"
                                | "output-type"
                                | "effect"
                                | "mutant-syntax"
                                | "name-collision"
                                | "source-span"
                        )
                    })
        }
        "noticed" | "proved" => {
            exact_keys(&model.answer, &["decision", "evidence"])
                && model.evidence.as_ref().is_some_and(|evidence| {
                    affirmative_evidence_shape(
                        evidence,
                        &model.mutant,
                        report_target,
                        i64::from(model.decision != "proved"),
                    )
                })
        }
        "undecided" => {
            exact_keys(&model.answer, &["decision", "attempt"])
                && model.attempt.as_ref().is_some_and(|attempt| {
                    exact_keys(attempt, &["reason", "evidence"])
                        && uncertainty_shape(attempt.get("reason"))
                        && attempt.get("evidence").is_some_and(|evidence| {
                            attempt_evidence_shape(
                                evidence,
                                attempt.get("reason"),
                                &model.mutant,
                                report_target,
                            )
                        })
                })
        }
        _ => false,
    }
}

fn affirmative_evidence_shape(
    evidence: &serde_json::Value,
    mutant: &str,
    report_target: &str,
    exit: i64,
) -> bool {
    exact_keys(
        evidence,
        &["verifier", "identity", "artifact", "source", "process"],
    ) && verifier_shape(evidence.get("verifier"), report_target)
        && identity_shape(evidence.get("identity"), mutant)
        && artifacts_shape(evidence, mutant, false)
        && process_shape(evidence.get("process"), Some(exit))
}

fn attempt_evidence_shape(
    evidence: &serde_json::Value,
    reason: Option<&serde_json::Value>,
    mutant: &str,
    report_target: &str,
) -> bool {
    if !exact_keys(
        evidence,
        &[
            "verifier",
            "identity",
            "artifact",
            "source",
            "process",
            "raw_sha256",
        ],
    ) || !identity_shape(evidence.get("identity"), mutant)
        || !artifacts_shape(evidence, mutant, true)
        || !process_shape(evidence.get("process"), None)
        || !digest_value(evidence.get("raw_sha256"))
    {
        return false;
    }
    let verifier = evidence.get("verifier");
    let artifact = evidence.get("artifact");
    if verifier.is_some_and(|value| !value.is_null())
        && (!verifier_shape(verifier, report_target)
            || artifact.is_none_or(serde_json::Value::is_null))
    {
        return false;
    }
    let raw = evidence
        .get("raw_sha256")
        .and_then(serde_json::Value::as_str);
    let raw_matches = match artifact.filter(|value| !value.is_null()) {
        Some(artifact) => artifact.get("sha256").and_then(serde_json::Value::as_str) == raw,
        None => raw == Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    };
    raw_matches && reason_process_shape(reason, evidence.get("process"))
}

fn verifier_shape(verifier: Option<&serde_json::Value>, report_target: &str) -> bool {
    let Some(verifier) = verifier else {
        return false;
    };
    if !exact_keys(verifier, &["tool", "backend"])
        || verifier.get("tool").and_then(serde_json::Value::as_str) != Some("0.68.0")
    {
        return false;
    }
    let Some(backend) = verifier.get("backend") else {
        return false;
    };
    exact_keys(
        backend,
        &[
            "export_version",
            "build_mode",
            "target",
            "rustc",
            "cbmc",
            "goto_cc",
            "goto_instrument",
            "solver",
        ],
    ) && backend
        .get("export_version")
        .and_then(serde_json::Value::as_str)
        == Some("1.0")
        && backend
            .get("build_mode")
            .and_then(serde_json::Value::as_str)
            == Some("release")
        && backend.get("target").and_then(serde_json::Value::as_str) == Some(report_target)
        && backend.get("rustc").and_then(serde_json::Value::as_str)
            == Some("rustc 1.100.0-nightly (8925ea358 2026-08-20)")
        && backend.get("cbmc").and_then(serde_json::Value::as_str) == Some("6.11.0 (cbmc-6.11.0)")
        && backend.get("goto_cc").and_then(serde_json::Value::as_str)
            == Some("clang version 21.0.0 (goto-cc 6.11.0 (cbmc-6.11.0))")
        && backend
            .get("goto_instrument")
            .and_then(serde_json::Value::as_str)
            == Some("6.11.0 (cbmc-6.11.0)")
        && backend.get("solver").and_then(serde_json::Value::as_str) == Some("cadical")
}

fn identity_shape(identity: Option<&serde_json::Value>, mutant: &str) -> bool {
    let Some(identity) = identity else {
        return false;
    };
    let keys = [
        "harness",
        "assertion",
        "unwind",
        "timeout_ms",
        "source_sha256",
        "rendered_sha256",
        "crate_input",
        "mutant",
        "path",
        "rule",
        "rule_version",
        "start_byte",
        "end_byte",
        "original_hex",
        "replacement_hex",
    ];
    let expected_harness = format!("__njutest_model_{mutant}");
    let expected_assertion = format!("njutest-model-v1:{mutant}");
    if !exact_keys(identity, &keys)
        || !digest_string(mutant)
        || identity.get("mutant").and_then(serde_json::Value::as_str) != Some(mutant)
        || identity.get("harness").and_then(serde_json::Value::as_str)
            != Some(expected_harness.as_str())
        || identity
            .get("assertion")
            .and_then(serde_json::Value::as_str)
            != Some(expected_assertion.as_str())
        || identity
            .get("unwind")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|value| value == 0 || value > u64::from(u32::MAX))
        || identity
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|value| value == 0)
        || !digest_value(identity.get("source_sha256"))
        || !digest_value(identity.get("rendered_sha256"))
        || !crate_input_shape(identity.get("crate_input"))
    {
        return false;
    }
    let Some(input) = mutation_identity_input(identity) else {
        return false;
    };
    match mint_mutant_id(&input.as_mint_input()) {
        Ok(minted) => minted == mutant,
        Err(MintMutantIdError::FieldTooLong { .. }) => false,
    }
}

struct ParsedMutationIdentity<'a> {
    path: &'a str,
    rule: &'a str,
    rule_version: u32,
    start: u32,
    end: u32,
    source_digest: &'a str,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

impl<'a> ParsedMutationIdentity<'a> {
    fn as_mint_input(&'a self) -> MutantIdentityInput<'a> {
        MutantIdentityInput {
            path: self.path,
            rule: self.rule,
            rule_version: self.rule_version,
            start: self.start,
            end: self.end,
            source_digest: self.source_digest,
            original: &self.original,
            replacement: &self.replacement,
        }
    }
}

fn mutation_identity_input(identity: &serde_json::Value) -> Option<ParsedMutationIdentity<'_>> {
    let path = identity.get("path")?.as_str()?;
    let rule = identity.get("rule")?.as_str()?;
    let rule_version = u32_value(identity.get("rule_version")?).filter(|value| *value > 0)?;
    let start = u32_value(identity.get("start_byte")?)?;
    let end = u32_value(identity.get("end_byte")?)?;
    let original = canonical_hex(identity.get("original_hex")?.as_str()?)?;
    let replacement = canonical_hex(identity.get("replacement_hex")?.as_str()?)?;
    let source_digest = identity.get("source_sha256")?.as_str()?;
    let original_length = match u64::try_from(original.len()) {
        Ok(length) => length,
        Err(_overflow) => return None,
    };
    if !canonical_workspace_path(path)
        || rule.is_empty()
        || rule.contains(['@', ' ', '\t', '\r', '\n'])
        || end.checked_sub(start).map(u64::from) != Some(original_length)
        || original == replacement
    {
        return None;
    }
    Some(ParsedMutationIdentity {
        path,
        rule,
        rule_version,
        start,
        end,
        source_digest,
        original,
        replacement,
    })
}

fn crate_input_shape(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    exact_keys(
        input,
        &[
            "package",
            "edition",
            "source",
            "offline",
            "dependency_resolution",
            "environment",
            "sha256",
        ],
    ) && input.get("package").and_then(serde_json::Value::as_str) == Some("njutest-verified-model")
        && input.get("edition").and_then(serde_json::Value::as_str) == Some("2024")
        && input.get("source").and_then(serde_json::Value::as_str) == Some("src/lib.rs")
        && input.get("offline").and_then(serde_json::Value::as_bool) == Some(true)
        && input
            .get("dependency_resolution")
            .and_then(serde_json::Value::as_str)
            == Some("empty-lock-offline-v1")
        && input.get("environment").and_then(serde_json::Value::as_str) == Some("minimal-v1")
        && digest_value(input.get("sha256"))
}

fn u32_value(value: &serde_json::Value) -> Option<u32> {
    let raw = value.as_u64()?;
    match u32::try_from(raw) {
        Ok(value) => Some(value),
        Err(_overflow) => None,
    }
}

/// Validates the published cross-platform spelling without consulting the producer's path normalizer.
/// Every component is already in its final form:
/// no separator conversion, dot elimination, or volume interpretation remains for a different host to perform.
fn canonical_workspace_path(path: &str) -> bool {
    if path.is_empty()
        || path.contains(['\0', '\\'])
        || path.starts_with('/')
        || matches!(
            (path.as_bytes().first(), path.as_bytes().get(1)),
            (Some(letter), Some(b':')) if letter.is_ascii_alphabetic()
        )
    {
        return false;
    }
    path.split('/')
        .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

struct MutantIdentityInput<'a> {
    path: &'a str,
    rule: &'a str,
    rule_version: u32,
    start: u32,
    end: u32,
    source_digest: &'a str,
    original: &'a [u8],
    replacement: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum MintMutantIdError {
    #[error("the {field} identity field exceeds the u32 length prefix")]
    FieldTooLong { field: &'static str },
}

fn mint_mutant_id(input: &MutantIdentityInput<'_>) -> Result<String, MintMutantIdError> {
    let mut hasher = sha2::Sha256::new();
    let version = input.rule_version.to_string();
    let start = input.start.to_string();
    let end = input.end.to_string();
    let original = digest_bytes(input.original);
    let replacement = digest_bytes(input.replacement);
    for (name, field) in [
        ("domain", "rust-mutants-id-v1"),
        ("path", input.path),
        ("rule", input.rule),
        ("rule-version", version.as_str()),
        ("start", start.as_str()),
        ("end", end.as_str()),
        ("source-digest", input.source_digest),
        ("original-digest", original.as_str()),
        ("replacement-digest", replacement.as_str()),
    ] {
        let length = u32::try_from(field.len())
            .map_err(|_overflow| MintMutantIdError::FieldTooLong { field: name })?;
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

fn digest_bytes(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

fn artifacts_shape(evidence: &serde_json::Value, mutant: &str, optional_raw: bool) -> bool {
    let source = evidence.get("source");
    if !artifact_shape(source, &format!("model/{mutant}.rs"))
        || source
            .and_then(|value| value.get("sha256"))
            .and_then(serde_json::Value::as_str)
            != evidence
                .get("identity")
                .and_then(|value| value.get("rendered_sha256"))
                .and_then(serde_json::Value::as_str)
    {
        return false;
    }
    let artifact = evidence.get("artifact");
    (optional_raw && artifact.is_some_and(serde_json::Value::is_null))
        || artifact_shape(artifact, &format!("model/{mutant}.json"))
}

fn artifact_shape(artifact: Option<&serde_json::Value>, expected_path: &str) -> bool {
    let Some(artifact) = artifact else {
        return false;
    };
    exact_keys(artifact, &["path", "bytes", "sha256"])
        && artifact.get("path").and_then(serde_json::Value::as_str) == Some(expected_path)
        && artifact
            .get("bytes")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|bytes| bytes > 0)
        && digest_value(artifact.get("sha256"))
}

fn process_shape(process: Option<&serde_json::Value>, expected_exit: Option<i64>) -> bool {
    let Some(process) = process else {
        return false;
    };
    let kind = process.get("kind").and_then(serde_json::Value::as_str);
    match kind {
        Some("exited") => {
            exact_keys(process, &["kind", "code"])
                && process
                    .get("code")
                    .and_then(serde_json::Value::as_i64)
                    .is_some()
                && expected_exit.is_none_or(|expected| {
                    process.get("code").and_then(serde_json::Value::as_i64) == Some(expected)
                })
        }
        Some("not-run" | "cutoff" | "cancelled" | "failed") => {
            expected_exit.is_none() && exact_keys(process, &["kind"])
        }
        Some(_) | None => false,
    }
}

fn reason_process_shape(
    reason: Option<&serde_json::Value>,
    process: Option<&serde_json::Value>,
) -> bool {
    let (Some(reason), Some(process)) = (reason, process) else {
        return false;
    };
    let kind = reason.get("kind").and_then(serde_json::Value::as_str);
    let detail = reason.get("detail").and_then(serde_json::Value::as_str);
    let process_kind = process.get("kind").and_then(serde_json::Value::as_str);
    let code = process.get("code").and_then(serde_json::Value::as_i64);
    match kind {
        Some("cutoff") => process_kind == Some("cutoff"),
        Some("cancelled") => process_kind == Some("cancelled"),
        Some("configuration") if detail == Some("workspace-drift") => true,
        Some("tool" | "configuration") => process_kind == Some("not-run"),
        Some("process") => process_kind == Some("failed"),
        Some("exit-mismatch") => {
            reason
                .get("detail")
                .and_then(|value| value.get("actual"))
                .and_then(serde_json::Value::as_i64)
                == code
        }
        Some("bound-exhausted" | "protocol" | "property" | "other-failure") => {
            process_kind == Some("exited") && matches!(code, Some(0 | 1))
        }
        Some("artifact") if detail == Some("source-changed") => true,
        Some("artifact") if detail == Some("already-exists") => process_kind == Some("not-run"),
        Some("artifact") => process_kind == Some("exited"),
        Some(_) | None => false,
    }
}

fn digest_value(value: Option<&serde_json::Value>) -> bool {
    value
        .and_then(serde_json::Value::as_str)
        .is_some_and(digest_string)
}

fn digest_string(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn canonical_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    match hex::decode(value) {
        Ok(bytes) => Some(bytes),
        Err(_error) => None,
    }
}

fn exact_keys(value: &serde_json::Value, expected: &[&str]) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn uncertainty_shape(reason: Option<&serde_json::Value>) -> bool {
    let Some(reason) = reason else {
        return false;
    };
    let Some(kind) = reason.get("kind").and_then(serde_json::Value::as_str) else {
        return false;
    };
    match (kind, uncertainty_details(kind)) {
        ("bound-exhausted" | "cutoff" | "cancelled", None) => exact_keys(reason, &["kind"]),
        (_, Some(details)) => tagged_detail(reason, details),
        ("other-failure", None) => {
            exact_keys(reason, &["kind", "detail"])
                && reason
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|detail| !detail.is_empty())
        }
        ("exit-mismatch", None) => {
            exact_keys(reason, &["kind", "detail"])
                && reason.get("detail").is_some_and(|detail| {
                    exact_keys(detail, &["expected", "actual"])
                        && matches!(
                            detail.get("expected").and_then(serde_json::Value::as_str),
                            Some("proved" | "noticed")
                        )
                        && detail
                            .get("actual")
                            .and_then(serde_json::Value::as_i64)
                            .is_some()
                })
        }
        _ => false,
    }
}

fn uncertainty_details(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "configuration" => Some(&[
            "package",
            "target",
            "profile",
            "compiler-flags",
            "compiler-environment",
            "relative-path",
            "directory",
            "tree-written",
            "workspace-drift",
        ]),
        "tool" => Some(&[
            "unavailable",
            "version-command",
            "version-banner",
            "harness-list-command",
            "harness-list-artifact",
            "harness-list-schema",
            "harness-list-match",
        ]),
        "process" => Some(&[
            "not-started",
            "stopped",
            "monitor",
            "wait",
            "signal",
            "unknown-exit",
            "unexpected-exit",
        ]),
        "artifact" => Some(&[
            "already-exists",
            "missing",
            "not-file",
            "too-large",
            "unreadable",
            "source-changed",
        ]),
        "protocol" => Some(&[
            "schema",
            "tool-version",
            "export-version",
            "backend",
            "summary",
            "harness",
            "assertion",
            "contradiction",
        ]),
        "property" => Some(&[
            "failure",
            "covered",
            "satisfied",
            "success",
            "undetermined",
            "unknown",
            "unreachable",
            "uncovered",
            "unsatisfiable",
            "error",
        ]),
        _ => None,
    }
}

fn tagged_detail(reason: &serde_json::Value, values: &[&str]) -> bool {
    exact_keys(reason, &["kind", "detail"])
        && reason
            .get("detail")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|detail| values.contains(&detail))
}

fn model_columns(recording: &Recording<'_>, notes: &mut Notes<'_>) {
    for (column_name, decision) in [("model_noticed", "noticed"), ("model_proved", "proved")] {
        notes.tally(Column {
            subject: &format!("accounting.mutants.{column_name}"),
            recorded: column(recording.document, "mutants", column_name),
            derived: size(
                recording
                    .models
                    .iter()
                    .filter(|model| model.decision == decision)
                    .count(),
            ),
            records: "the affirmative model records carrying that decision",
        });
    }
}

/// Whether any layer removed a target that then killed the mutation it removed.
/// The targets the recording says noticed nothing, held to the findings that name them.
///
/// Re-derived from the executions alone.
/// A target is asked about a mutation only after every target before it in the route survived it, so every execution the recording holds is one where that target had its chance —
/// except one nobody decided, which is a chance the run could not give it and is left out of the count rather than held against it.
///
/// A part of a catalog is not held to this at all.
/// Whether a target notices anything is a statement about the whole catalog, and a part has seen a slice: a target silent in this part may have noticed something in another,
/// and demanding a finding here would demand one the whole would contradict.
fn hollow(recording: &Recording<'_>, routing: Option<&crate::route::Routing>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Hollow);
    if recording.shard.is_some() {
        return;
    }
    let Some(routing) = routing else {
        notes.unaudited(
            "executions",
            "the run kept no recording of what it ran, so which targets were put to a \
             mutation and noticed none cannot be re-derived"
                .to_owned(),
        );
        return;
    };
    if routing.execs.is_empty() {
        notes.unaudited(
            "executions",
            "the recording holds no mutation execution, so no target was put to anything \
             this audit could hold it to"
                .to_owned(),
        );
        return;
    }
    let asked = match asked_targets(routing) {
        Ok(asked) => asked,
        Err(overflow) => {
            notes.violated(
                overflow.target,
                "the execution count exceeds the report wire's u64 range".to_owned(),
            );
            return;
        }
    };
    let owed: BTreeSet<&str> = asked
        .iter()
        .filter(|(_, (count, noticed))| *count > 0 && !*noticed)
        .map(|(target, _)| *target)
        .collect();
    let named = named_hollow_targets(recording);
    for target in owed.difference(&named) {
        let count = match asked.get(target) {
            Some((count, _noticed)) => *count,
            None => {
                notes.violated(
                    target,
                    "the independently derived hollow-target set lost its execution count"
                        .to_owned(),
                );
                continue;
            }
        };
        notes.violated(
            target,
            format!(
                "{target} answered about {count} mutation(s) and noticed none of them, \
                 and the report names no hollow-target finding about it"
            ),
        );
    }
    for target in named.difference(&owed) {
        notes.violated(
            target,
            format!(
                "the report calls {target} hollow, and the recording has it noticing \
                 something or answering about nothing"
            ),
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HollowCountOverflow<'a> {
    target: &'a str,
}

fn asked_targets(
    routing: &crate::route::Routing,
) -> Result<BTreeMap<&str, (u64, bool)>, HollowCountOverflow<'_>> {
    let mut asked = BTreeMap::new();
    for exec in &routing.execs {
        if !matches!(exec.outcome.as_str(), "killed" | "survived") {
            continue;
        }
        let target = exec.target.as_str();
        let held = asked.entry(target).or_insert((0_u64, false));
        held.0 = held
            .0
            .checked_add(1)
            .ok_or(HollowCountOverflow { target })?;
        held.1 |= exec.outcome == KILLED;
    }
    Ok(asked)
}

fn named_hollow_targets<'a>(recording: &'a Recording<'_>) -> BTreeSet<&'a str> {
    let Some(findings) = recording
        .document
        .get("findings")
        .and_then(serde_json::Value::as_array)
    else {
        return BTreeSet::new();
    };
    findings
        .iter()
        .filter(|one| one.get("kind").and_then(serde_json::Value::as_str) == Some("hollow-target"))
        .filter_map(|one| one.get("subject").and_then(serde_json::Value::as_str))
        .collect()
}

/// The faults a seam recording licensed, re-derived here and held to what the run says became of them.
///
/// The catalogue is minted again from the exchanges alone, by the rules and the identity recipe written out in `crate::wire`, so a fault this audit does not derive is one the run invented and a fault it derives that the run never put is a question the report is quiet about.
fn wire(recording: &Recording<'_>, watched: Option<&crate::wire::Watched>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Wire);
    let Some(watched) = watched else {
        return;
    };
    if watched.exchanges.is_empty() && watched.execs.is_empty() {
        return;
    }
    let mut owed: BTreeMap<String, String> = BTreeMap::new();
    for exchange in &watched.exchanges {
        let licensed = match crate::wire::licensed(exchange) {
            Ok(licensed) => licensed,
            Err(error) => {
                let subject = format!("{}:{}", exchange.capability, exchange.seq);
                notes.violated(
                    &subject,
                    format!("the exchange cannot mint its prescribed fault identities: {error}"),
                );
                continue;
            }
        };
        for (id, rule) in licensed {
            owed.insert(id, rule);
        }
    }
    let put: BTreeMap<&str, &crate::wire::Exec> = watched
        .execs
        .iter()
        .map(|one| (one.fault.as_str(), one))
        .collect();
    if put.is_empty() {
        notes.unaudited(
            "faults",
            format!(
                "{} exchange(s) went past a seam and licensed {} question(s), and the \
                 recording holds none of them being put, so what the suite would have \
                 done with them cannot be re-derived",
                watched.exchanges.len(),
                owed.len()
            ),
        );
        return;
    }
    for (id, rule) in &owed {
        if !put.contains_key(id.as_str()) {
            notes.violated(
                id,
                format!(
                    "the exchanges the recording holds license {rule} here, and the run \
                     records neither putting it nor why it did not"
                ),
            );
        }
    }
    for (id, exec) in &put {
        if !owed.contains_key(*id) {
            notes.violated(
                id,
                format!(
                    "the run put {} on the {} seam at exchange {}, and no exchange this \
                     audit re-derives from the recording licenses it",
                    exec.rule, exec.capability, exec.seq
                ),
            );
        }
    }
    gaps(recording, &put, &mut notes);
}

/// What each fault site came to, re-derived from the recording's fault executions and held to the report (ADR 0032).
fn faults(recording: &Recording<'_>, faulted: Option<&crate::faults::Faulted>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Faults);
    let reported: Vec<crate::faults::Site> = rows(recording.document, "faults")
        .iter()
        .map(crate::faults::site)
        .collect();
    fault_counts(recording, &reported, &mut notes);
    fault_findings(recording, &reported, &mut notes);
    let Some(faulted) = faulted else {
        if reported.is_empty() {
            return;
        }
        notes.unaudited(
            "faults",
            format!(
                "the report holds {} fault site(s) and there is no recording to re-derive them from",
                reported.len()
            ),
        );
        return;
    };
    broken(recording, faulted, &mut notes);
    let recorded: BTreeMap<&str, &crate::faults::Site> = faulted
        .sites
        .iter()
        .map(|site| (site.fault.as_str(), site))
        .collect();
    for site in &faulted.sites {
        if !reported.iter().any(|one| one.fault == site.fault) {
            notes.violated(
                &site.fault,
                "the recording holds this fault site and the report does not".to_owned(),
            );
        }
    }
    for site in &reported {
        if recorded.get(site.fault.as_str()) != Some(&site) {
            notes.violated(
                &site.fault,
                "the report's record of this fault is not the one the recording holds".to_owned(),
            );
        }
        if let Err(why) = crate::faults::supports(site, &faulted.evidence(&site.fault)) {
            notes.violated(&site.fault, why.to_string());
        }
    }
}

/// The fault counts, re-derived from the report's own records, since a part whose six decisions do not add up to its sites is refused whatever the recording says.
fn fault_counts(
    recording: &Recording<'_>,
    reported: &[crate::faults::Site],
    notes: &mut Notes<'_>,
) {
    for (column_name, decision) in [
        ("noticed", "noticed"),
        ("unnoticed", "unnoticed"),
        ("unreached", "unreached"),
        ("waited", "waited"),
        ("undecided", "undecided"),
        ("not_put", "not-put"),
    ] {
        let expected = reported
            .iter()
            .filter(|site| site.decision == decision)
            .count();
        if column(recording.document, "faults", column_name) != size(expected) {
            notes.violated(
                column_name,
                format!(
                    "the report's fault records hold {expected} {decision} site(s), and its \
                     count says otherwise"
                ),
            );
        }
    }
    if column(recording.document, "faults", "sites") != size(reported.len()) {
        notes.violated(
            "sites",
            format!(
                "the report holds {} fault record(s), and its count of sites says otherwise",
                reported.len()
            ),
        );
    }
}

/// The failures nothing noticed, held to the findings that name them, in both directions.
/// Every `broken-under-fault` finding, held to the attribution that ties its write to that fault, and every such attribution to a finding.
fn broken(recording: &Recording<'_>, faulted: &crate::faults::Faulted, notes: &mut Notes<'_>) {
    let named: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|finding| finding.kind == BROKEN_UNDER_FAULT)
        .map(|finding| finding.subject.as_str())
        .collect();
    let tied: BTreeSet<&str> = faulted.attributed.iter().map(String::as_str).collect();
    for fault in named.difference(&tied) {
        notes.violated(
            fault,
            "a finding says this fault wrote into the tree, and the recording holds no run of it \
             alone that wrote while its test passed where the test alone without it did not"
                .to_owned(),
        );
    }
    for fault in tied.difference(&named) {
        notes.violated(
            fault,
            "the recording ties a write into the tree to this fault, and no broken-under-fault \
             finding says so"
                .to_owned(),
        );
    }
}

fn fault_findings(
    recording: &Recording<'_>,
    reported: &[crate::faults::Site],
    notes: &mut Notes<'_>,
) {
    let owed: BTreeMap<&str, &str> = reported
        .iter()
        .filter_map(|site| {
            let kind = match site.decision.as_str() {
                "unnoticed" => UNNOTICED_FAULT,
                "waited" | "undecided" => NOT_MEASURED_FINDING,
                _ => return None,
            };
            Some((site.fault.as_str(), kind))
        })
        .collect();
    let sites: BTreeSet<&str> = reported.iter().map(|site| site.fault.as_str()).collect();
    let named: BTreeSet<(&str, &str)> = recording
        .findings
        .iter()
        .filter(|finding| {
            finding.kind == UNNOTICED_FAULT
                || (finding.kind == NOT_MEASURED_FINDING
                    && sites.contains(finding.subject.as_str()))
        })
        .map(|finding| (finding.subject.as_str(), finding.kind.as_str()))
        .collect();
    for (fault, kind) in &owed {
        if !named.contains(&(*fault, *kind)) {
            notes.violated(
                fault,
                format!(
                    "the report's decision about this fault owes a {kind} finding, and none says so"
                ),
            );
        }
    }
    for (fault, kind) in &named {
        if owed.get(fault) != Some(kind) {
            notes.violated(
                fault,
                format!(
                    "a {kind} finding names this fault, and the report records no decision about \
                     it that owes one"
                ),
            );
        }
    }
}

/// The questions the recording says nothing noticed, held to the findings that name them.
fn gaps(
    recording: &Recording<'_>,
    put: &BTreeMap<&str, &crate::wire::Exec>,
    notes: &mut Notes<'_>,
) {
    let named: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|one| one.kind == "wire-unnoticed")
        .map(|one| one.subject.as_str())
        .collect();
    for (id, exec) in put {
        let unnoticed = exec.decision == "unnoticed";
        if unnoticed && !named.contains(*id) {
            notes.violated(
                id,
                format!(
                    "the recording has nothing noticing {} on the {} seam, and the report \
                     names no wire-unnoticed finding about it",
                    exec.rule, exec.capability
                ),
            );
        }
        if !unnoticed && named.contains(*id) {
            notes.violated(
                id,
                format!(
                    "the report calls this a gap, and the recording has it decided by {}",
                    exec.decision
                ),
            );
        }
    }
    for id in named {
        if !put.contains_key(id) {
            notes.violated(
                id,
                "the report names a question nothing noticed, and the recording has no \
                 run putting it"
                    .to_owned(),
            );
        }
    }
}

fn proofs(recording: &Recording<'_>, routing: Option<&crate::route::Routing>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let Some(routing) = routing else {
        notes.unaudited(
            "route",
            "the run kept no recording of how it routed, so which target each proof removed \
             cannot be re-derived"
                .to_owned(),
        );
        return;
    };
    let removed: BTreeMap<String, BTreeMap<String, String>> = routing
        .routes
        .iter()
        .map(|route| {
            let discharged = route
                .discharged
                .iter()
                .map(|one| (one.target.clone(), one.proof.clone()))
                .collect();
            (route.mutant.clone(), discharged)
        })
        .collect();
    let executed: BTreeSet<String> = routing
        .execs
        .iter()
        .map(|exec| exec.mutant.clone())
        .collect();
    let ran: Vec<(String, String, String)> = routing
        .execs
        .iter()
        .map(|exec| {
            (
                exec.mutant.clone(),
                exec.target.clone(),
                exec.outcome.clone(),
            )
        })
        .collect();
    if removed.is_empty() && ran.is_empty() {
        notes.unaudited(
            "route",
            "the recording holds no routing decision and no mutation execution, so there is \
             nothing to hold a layer to"
                .to_owned(),
        );
        return;
    }
    let known: BTreeSet<&str> = recording
        .targets
        .iter()
        .map(|target| target.name.as_str())
        .collect();
    discharges(&removed, &ran, &mut notes);
    kept(&routing.routes, &ran, &mut notes);
    reach(&routing.routes, &known, &executed, &mut notes);
    believed(&routing.routes, &mut notes);
}

/// Reuse, re-derived: a route names the run whose answer it took, or why it took none, and never both.
fn believed(routes: &[crate::route::Route], notes: &mut Notes<'_>) {
    for route in routes {
        if let (Some(run), Some(refusal)) = (route.reused.as_ref(), route.refused.as_ref()) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says the answer was read back from {run} and that it was \
                     refused as {refusal}; one of those is not what happened, and a \
                     recording that says both cannot be held to either"
                ),
            );
        }
    }
}

/// Every proof that removed a target, against the kills the recording holds: a layer that drops a target which then finds a defect is unsound.
fn discharges(
    removed: &BTreeMap<String, BTreeMap<String, String>>,
    ran: &[(String, String, String)],
    notes: &mut Notes<'_>,
) {
    for (mutant, target, outcome) in ran {
        if outcome != KILLED {
            continue;
        }
        let Some(proof) = removed.get(mutant).and_then(|one| one.get(target)) else {
            continue;
        };
        notes.violated(
            mutant,
            format!(
                "{proof} removed {target} from what could notice this mutation, and {target} \
                 then {outcome} it; a layer that drops a target which finds a defect is unsound"
            ),
        );
    }
}

/// Every kill, against the route that decided which targets would be asked: a layer that drops a target which then finds a defect is unsound, however it dropped it.
fn kept(routes: &[crate::route::Route], ran: &[(String, String, String)], notes: &mut Notes<'_>) {
    for (mutant, target, outcome) in ran {
        if outcome != KILLED {
            continue;
        }
        let Some(route) = routes
            .iter()
            .find(|route| &route.mutant == mutant && route.reused.is_none())
        else {
            continue;
        };
        if route.reaching.is_empty() || route.reaching.iter().any(|one| one == target) {
            continue;
        }
        if route.discharges(target) {
            continue;
        }
        notes.violated(
            mutant,
            format!(
                "the route did not keep {target} for this mutation and the recording then \
                 shows {target} {outcome} it; a target the measurement placed elsewhere is \
                 a target the reach layer removed, and a layer that removes one which \
                 finds a defect is unsound"
            ),
        );
    }
}

/// The reach layer, re-derived from what the route named rather than confirmed from what it decided.
fn reach(
    routes: &[crate::route::Route],
    known: &BTreeSet<&str>,
    executed: &BTreeSet<String>,
    notes: &mut Notes<'_>,
) {
    for route in routes {
        if route.reused.is_some() {
            continue;
        }
        let ran = executed.contains(&route.mutant);
        if route.granularity == UNREACHED {
            if ran {
                notes.violated(
                    &route.mutant,
                    "the route says no measured target reaches this mutation and the \
                     recording then runs one against it; a claim about the code that its \
                     own run contradicts is not a claim"
                        .to_owned(),
                );
            }
            if route.considered.is_empty() {
                notes.violated(
                    &route.mutant,
                    "the route removed every execution and named no target it removed \
                     them from; nothing reaches a place only if somebody was in a \
                     position to notice and did not, and this says nobody was"
                        .to_owned(),
                );
            }
        }
        if route.granularity == EVERYTHING && !ran {
            notes.violated(
                &route.mutant,
                "the route says the measurement does not carry which targets reach this \
                 mutation, and the recording runs nothing against it; a premise that \
                 fails has to end in more work rather than in less"
                    .to_owned(),
            );
        }
        considered(route, known, notes);
    }
}

/// Every target a route says was asked and did not reach, against the run that says which targets there were.
fn considered(route: &crate::route::Route, known: &BTreeSet<&str>, notes: &mut Notes<'_>) {
    for target in &route.considered {
        if !known.contains(target.as_str()) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says {target} was measured and did not reach this \
                     mutation, and the run reports no such target; a layer held to \
                     targets that are not there is held to nothing"
                ),
            );
        }
        if route.reaching.contains(target) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route both keeps {target} for this mutation and says it did not \
                     reach it; one route cannot answer a question two ways"
                ),
            );
        }
        if route.discharges(target) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says {target} did not reach this mutation and also names \
                     a proof that removed it; a target that reaches nothing needs no \
                     proof, and a proof that removed it says it did reach"
                ),
            );
        }
    }
}

#[derive(Debug)]
struct Recording<'a> {
    document: &'a serde_json::Value,
    run_id: String,
    contract: String,
    targets: Vec<TargetRow>,
    mutants: Vec<MutantRow>,
    findings: Vec<FindingRow>,
    shard: Option<String>,
    target: String,
    models: Vec<ModelRow>,
}

impl<'a> Recording<'a> {
    fn of(document: &'a serde_json::Value) -> Self {
        Self {
            document,
            run_id: field(document, "run_id").unwrap_or_default(),
            contract: field(document, "contract").unwrap_or_default(),
            targets: rows(document, "targets")
                .iter()
                .map(|row| TargetRow {
                    id: field(row, "id").unwrap_or_default(),
                    name: field(row, "name").unwrap_or_default(),
                    status: field(row, "status").unwrap_or_default(),
                })
                .collect(),
            mutants: rows(document, "mutants")
                .iter()
                .map(|row| {
                    let decision = row
                        .get("decision")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let reuse = row.get("reuse").cloned().unwrap_or(serde_json::Value::Null);
                    MutantRow {
                        id: field(row, "id").unwrap_or_default(),
                        display_id: field(row, "display_id").unwrap_or_default(),
                        outcome: field(&decision, "outcome").unwrap_or_default(),
                        acceptance: AcceptanceFact::from_json(row.get("accepted")),
                        killed_by: field(&decision, "killed_by"),
                        reused: reuse
                            .get("reused")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or_default(),
                        source_run_id: field(&reuse, "source_run_id"),
                    }
                })
                .collect(),
            findings: rows(document, "findings")
                .iter()
                .map(|row| FindingRow {
                    kind: field(row, "kind").unwrap_or_default(),
                    subject: field(row, "subject").unwrap_or_default(),
                })
                .collect(),
            shard: document
                .get("scope")
                .and_then(|scope| field(scope, "shard")),
            target: document
                .get("toolchain")
                .and_then(|toolchain| field(toolchain, "target"))
                .unwrap_or_default(),
            models: rows(document, "models")
                .iter()
                .map(|row| {
                    let answer = row
                        .get("answer")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    ModelRow {
                        mutant: field(row, "mutant").unwrap_or_default(),
                        decision: field(&answer, "decision").unwrap_or_default(),
                        evidence: answer.get("evidence").cloned(),
                        attempt: answer.get("attempt").cloned(),
                        answer,
                        raw: row.clone(),
                    }
                })
                .collect(),
        }
    }

    fn dispositions(&self, outcome: &str) -> usize {
        self.mutants
            .iter()
            .filter(|mutant| mutant.outcome == outcome)
            .count()
    }
}

/// Whether the target columns say what the target records say.
fn target_columns(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.tally(Column {
        subject: "accounting.targets.selected",
        recorded: column(recording.document, "targets", "selected"),
        derived: size(recording.targets.len()),
        records: "the target records it carries",
    });
    for status in [PASSED, "failed", "skipped", "missing"] {
        let recorded = recording
            .targets
            .iter()
            .filter(|target| target.status == status)
            .count();
        notes.tally(Column {
            subject: &format!("accounting.targets.{status}"),
            recorded: column(recording.document, "targets", status),
            derived: size(recorded),
            records: "the target records carrying that status",
        });
    }
}

/// Whether the mutant columns say what the mutant records say.
fn mutant_columns(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.tally(Column {
        subject: "accounting.mutants.cataloged",
        recorded: column(recording.document, "mutants", "cataloged"),
        derived: size(recording.mutants.len()),
        records: "the mutant records it carries",
    });
    for (name, outcome) in [
        ("rejected", REJECTED),
        (KILLED, KILLED),
        (SURVIVED, SURVIVED),
        ("step_limit_reached", STEP_LIMIT_REACHED),
        (WAITED, WAITED),
        (UNREACHED, UNREACHED),
        (EQUIVALENT, EQUIVALENT),
    ] {
        notes.tally(Column {
            subject: &format!("accounting.mutants.{name}"),
            recorded: column(recording.document, "mutants", name),
            derived: size(recording.dispositions(outcome)),
            records: "the mutant records carrying that disposition",
        });
    }
    let excluded = recording
        .dispositions(REJECTED)
        .checked_add(recording.dispositions(UNREACHED))
        .and_then(|count| count.checked_add(recording.dispositions(EQUIVALENT)));
    let ran = excluded.and_then(|excluded| recording.mutants.len().checked_sub(excluded));
    notes.tally(Column {
        subject: "accounting.mutants.executed",
        recorded: column(recording.document, "mutants", "executed"),
        derived: ran.and_then(size),
        records: "the mutant records the compiler accepted and something reached",
    });
    for (name, outcome) in [("reused_killed", KILLED), ("reused_survived", SURVIVED)] {
        let recorded = recording
            .mutants
            .iter()
            .filter(|mutant| mutant.outcome == outcome && mutant.reused)
            .count();
        notes.tally(Column {
            subject: &format!("accounting.mutants.{name}"),
            recorded: column(recording.document, "mutants", name),
            derived: size(recorded),
            records: "the mutant records of that disposition read back from an earlier run",
        });
    }
    let accepted = recording
        .mutants
        .iter()
        .filter(|mutant| mutant.acceptance == AcceptanceFact::Accepted)
        .count();
    notes.tally(Column {
        subject: "accounting.mutants.accepted",
        recorded: column(recording.document, "mutants", "accepted"),
        derived: size(accepted),
        records: "the mutant rows carrying their own acceptance",
    });
    for mutant in &recording.mutants {
        match mutant.acceptance {
            AcceptanceFact::Missing => notes.violated(
                mutant.label(),
                "the current report row omits its required acceptance fact".to_owned(),
            ),
            AcceptanceFact::Accepted
                if !matches!(mutant.outcome.as_str(), SURVIVED | UNREACHED | EQUIVALENT) =>
            {
                notes.violated(
                    mutant.label(),
                    format!(
                        "outcome {} cannot be answered by a review acceptance",
                        mutant.outcome
                    ),
                );
            }
            AcceptanceFact::Rejected | AcceptanceFact::Accepted => {}
        }
    }
}

/// Whether the columns add up the way the assurance contract says they do, which a recording carrying no records at all must still satisfy.
fn equations(recording: &Recording<'_>, audit: &mut Audit) {
    let targets = |name: &str| column(recording.document, "targets", name);
    let mutants = |name: &str| column(recording.document, "mutants", name);
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.holds(Equation {
        subject: "accounting.targets",
        relation: Relation::Exactly,
        sides: (
            sum(&[
                targets(PASSED),
                targets("failed"),
                targets("skipped"),
                targets("missing"),
            ]),
            targets("selected"),
        ),
        because: "every selected target has exactly one terminal state",
    });
    notes.holds(Equation {
        subject: "accounting.mutants",
        relation: Relation::Exactly,
        sides: (
            sum(&[
                mutants("rejected"),
                mutants("executed"),
                mutants(UNREACHED),
                mutants(EQUIVALENT),
            ]),
            mutants("cataloged"),
        ),
        because: "every cataloged mutant was refused by the compiler, reached by nothing, \
                  rendered identically to what it mutates, or executed",
    });
    notes.holds(Equation {
        subject: "accounting.mutants.executed",
        relation: Relation::AtMost,
        sides: (
            sum(&[
                mutants(KILLED),
                mutants(SURVIVED),
                mutants("step_limit_reached"),
                mutants(WAITED),
            ]),
            mutants("executed"),
        ),
        because: "a mutation a test noticed, one nothing noticed, and each non-verdict \
                  execution boundary were all executed",
    });
    notes.holds(Equation {
        subject: "accounting.mutants.accepted",
        relation: Relation::AtMost,
        sides: (
            mutants("accepted"),
            sum(&[mutants(SURVIVED), mutants(UNREACHED), mutants(EQUIVALENT)]),
        ),
        because: "an acceptance is a reviewer's answer to a mutation nothing noticed, which is \
                  one nothing reached as much as one every reaching test passed",
    });
    for (name, whole) in [("reused_killed", KILLED), ("reused_survived", SURVIVED)] {
        notes.holds(Equation {
            subject: &format!("accounting.mutants.{name}"),
            relation: Relation::AtMost,
            sides: (mutants(name), mutants(whole)),
            because: "a reused disposition is one of that column, not an addition to it",
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum Relation {
    Exactly,
    AtMost,
}

impl Relation {
    const fn word(self) -> &'static str {
        match self {
            Self::Exactly => "exactly",
            Self::AtMost => "at most",
        }
    }

    const fn holds(self, left: u64, right: u64) -> bool {
        match self {
            Self::Exactly => left == right,
            Self::AtMost => left <= right,
        }
    }
}

/// Whether the verdict is one the accounting and the findings support.
fn verdict(recording: &Recording<'_>, audit: &mut Audit) {
    let Some(concluded) = field(recording.document, "verdict") else {
        Notes::on(audit, Layer::Accounting).unaudited(
            "verdict",
            "the recording does not say what the run concluded, so there is nothing to hold its \
             accounting to"
                .to_owned(),
        );
        return;
    };
    match concluded.as_str() {
        "ASSURED" | "CHANGE_ASSURED" | "SCOPE_ASSURED" => assurance(recording, audit, &concluded),
        "DEFECT" => defect(recording, audit),
        _ => {}
    }
}

fn assurance(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) {
    scope(recording, audit, concluded);
    let mut notes = Notes::on(audit, Layer::Accounting);
    let unsupported = |notes: &mut Notes<'_>, because: &str| {
        notes.violated(
            "verdict",
            format!("the recording concludes {concluded} and {because}"),
        );
    };
    if !recording.findings.is_empty() {
        unsupported(
            &mut notes,
            &format!(
                "carries {} findings; an assurance is the claim that nothing was found",
                recording.findings.len()
            ),
        );
    }
    for (name, because) in [
        ("failed", "a failing test is not an assurance"),
        (
            "missing",
            "a target that never ran cannot support a claim about what it would have said",
        ),
    ] {
        if let Some(count) = column(recording.document, "targets", name)
            && count > 0
        {
            unsupported(
                &mut notes,
                &format!("counts {count} {name} targets; {because}"),
            );
        }
    }
    for (group, name, because) in [
        (
            "targets",
            PASSED,
            "nothing was observed, so nothing is assured",
        ),
        (
            "mutants",
            "executed",
            "nothing was asked of the tests, so nothing about them is assured",
        ),
    ] {
        if column(recording.document, group, name) == Some(0) {
            unsupported(&mut notes, &format!("counts no {name} {group}; {because}"));
        }
    }
    for mutant in &recording.mutants {
        let answered = matches!(
            (mutant.outcome.as_str(), mutant.acceptance),
            (
                REJECTED | KILLED | "model-noticed" | "model-proved" | EQUIVALENT,
                AcceptanceFact::Rejected | AcceptanceFact::Accepted
            ) | (SURVIVED | UNREACHED, AcceptanceFact::Accepted)
        );
        if !answered {
            unsupported(
                &mut notes,
                &format!(
                    "mutation {} ended as {} without a row-local answer",
                    mutant.label(),
                    mutant.outcome
                ),
            );
        }
    }
}

/// An assurance reaches exactly as far as the run looked, and the recording says how far that was.
fn scope(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    let Some(kind) = field(recording.document, "run_kind") else {
        notes.unaudited(
            "verdict",
            "the recording does not say how much of the workspace the run looked at, so the \
             reach of its assurance cannot be re-decided"
                .to_owned(),
        );
        return;
    };
    let reached = match kind.as_str() {
        "full" => "ASSURED",
        "changed" => "CHANGE_ASSURED",
        "scoped" => "SCOPE_ASSURED",
        _ => {
            notes.unaudited(
                "verdict",
                format!(
                    "the recording names the scope {kind:?}, which this audit knows no assurance \
                     to hold it to"
                ),
            );
            return;
        }
    };
    if reached != concluded {
        notes.violated(
            "verdict",
            format!(
                "the recording concludes {concluded} over a {kind} run, which assures only what \
                 it looked at, and that is {reached}"
            ),
        );
    }
}

fn defect(recording: &Recording<'_>, audit: &mut Audit) {
    if !recording
        .findings
        .iter()
        .any(|finding| DEFECT_KINDS.contains(&finding.kind.as_str()))
    {
        Notes::on(audit, Layer::Accounting).violated(
            "verdict",
            "the recording concludes DEFECT and names nothing wrong with the code under test; a \
             surviving mutation is a gap in the tests, and a fault a reader cannot see named is \
             not one they can act on"
                .to_owned(),
        );
    }
}

/// Whether every kill names a target this run itself saw pass on the original tree.
fn killers(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Killers);
    for mutant in recording
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome == KILLED)
    {
        let Some(killer) = mutant.killed_by.as_deref() else {
            notes.violated(
                mutant.label(),
                "the recording says a test noticed this mutation and does not say which; a kill \
                 nobody can name is not a kill a reader can check"
                    .to_owned(),
            );
            continue;
        };
        let Some(target) = recording
            .targets
            .iter()
            .find(|target| target.name == killer || target.id == killer)
        else {
            notes.violated(
                mutant.label(),
                format!(
                    "the kill is attributed to {killer:?}, which is not among the targets this \
                     run recorded"
                ),
            );
            continue;
        };
        if target.status != PASSED {
            notes.violated(
                mutant.label(),
                format!(
                    "the kill is attributed to {killer:?}, which this run recorded as {} on the \
                     original tree; a target it never saw pass there cannot tell the two \
                     programs apart",
                    target.status
                ),
            );
        }
    }
}

/// Whether the mutations nothing noticed and the findings that raise them are the same set.
fn findings(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Findings);
    let mutation_kinds = [
        SURVIVING_MUTANT,
        WAITED_MUTANT,
        STEP_LIMIT_REACHED_MUTANT,
        FAILING_TEST,
        TARGET_MISSING,
    ];
    for mutant in &recording.mutants {
        let expected = match (mutant.outcome.as_str(), mutant.acceptance) {
            (_, AcceptanceFact::Missing) => continue,
            (SURVIVED | UNREACHED, AcceptanceFact::Rejected) => Some(SURVIVING_MUTANT),
            (STEP_LIMIT_REACHED, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => {
                Some(STEP_LIMIT_REACHED_MUTANT)
            }
            (WAITED, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => Some(WAITED_MUTANT),
            (UNCONFIRMED, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => {
                Some(FAILING_TEST)
            }
            (ERRORED, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => Some(TARGET_MISSING),
            (_, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => None,
        };
        let tied: Vec<&FindingRow> = recording
            .findings
            .iter()
            .filter(|finding| mutation_kinds.contains(&finding.kind.as_str()))
            .filter(|finding| mutant.answers_to(&finding.subject))
            .collect();
        match expected {
            Some(kind) if matches!(tied.as_slice(), [finding] if finding.kind == kind) => {}
            Some(kind) => notes.violated(
                mutant.label(),
                format!(
                    "outcome {} requires exactly one {kind} finding, but {} mutation finding(s) name it",
                    mutant.outcome,
                    tied.len()
                ),
            ),
            None if tied.is_empty() => {}
            None => notes.violated(
                mutant.label(),
                format!(
                    "outcome {} requires no mutation finding, but {} mutation finding(s) name it",
                    mutant.outcome,
                    tied.len()
                ),
            ),
        }
    }
    for finding in &recording.findings {
        if matches!(
            finding.kind.as_str(),
            SURVIVING_MUTANT | WAITED_MUTANT | STEP_LIMIT_REACHED_MUTANT
        ) && !recording
            .mutants
            .iter()
            .any(|mutant| mutant.answers_to(&finding.subject))
        {
            notes.violated(
                &finding.subject,
                "the mutation finding names no mutation row in the recording".to_owned(),
            );
        }
    }
}

/// Whether a finding that calls an acceptance unmatched is supported by the complete catalog.
fn acceptances(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Acceptances);
    for finding in recording
        .findings
        .iter()
        .filter(|finding| finding.kind == UNMATCHED_ACCEPTANCE)
    {
        if recording.shard.is_some() {
            notes.unaudited(
                &finding.subject,
                "this shard does not carry the complete catalog, so whether the acceptance \
                 resolves uniquely is decided only after the shards are merged"
                    .to_owned(),
            );
            continue;
        }
        let subject = finding.subject.as_str();
        let valid = (4..=64).contains(&subject.len())
            && subject
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            continue;
        }
        let matches: Vec<&MutantRow> = recording
            .mutants
            .iter()
            .filter(|mutant| mutant.id.starts_with(subject))
            .collect();
        if let [mutant] = matches.as_slice() {
            notes.violated(
                subject,
                format!(
                    "the finding calls this acceptance unmatched, but it uniquely resolves to \
                     {}; an unmatched acceptance must suppress nothing",
                    mutant.id
                ),
            );
        }
    }
}

/// Whether every disposition read back from an earlier run names one a reader could go and read.
fn reuse(recording: &Recording<'_>, audit: &mut Audit) {
    let mut read_back = 0_usize;
    let mut notes = Notes::on(audit, Layer::Reuse);
    for mutant in &recording.mutants {
        match (mutant.reused, mutant.source_run_id.as_deref()) {
            (true, None) => notes.violated(
                mutant.label(),
                "the disposition was read back from an earlier run and does not name it; a \
                 verdict a reader cannot trace back is one taken on trust"
                    .to_owned(),
            ),
            (true, Some(run)) if run == recording.run_id => notes.violated(
                mutant.label(),
                "the disposition names this run itself as the run it was read back from; a run \
                 cannot have read its own answer back"
                    .to_owned(),
            ),
            (true, Some(_)) => match read_back.checked_add(1) {
                Some(count) => read_back = count,
                None => {
                    notes.violated(
                        "provenance",
                        "the number of reused dispositions exceeds usize".to_owned(),
                    );
                    return;
                }
            },
            (false, Some(run)) => notes.violated(
                mutant.label(),
                format!(
                    "this run established the disposition itself and also names {run:?} as the \
                     run it came from; one of the two is wrong and a reader cannot tell which"
                ),
            ),
            (false, None) => {}
        }
    }
    if read_back > 0 {
        notes.unaudited(
            "provenance",
            format!(
                "{read_back} dispositions were read back from an earlier run; whether the \
                 recorded target is still routed to the mutation under the same behaviour key is \
                 a fact this report does not carry"
            ),
        );
    }
}

/// A string the recording says something in.
/// Whitespace is nothing to say, and reads here as the absent value it is.
fn field(value: &serde_json::Value, key: &str) -> Option<String> {
    let said = value.get(key)?.as_str()?.trim();
    (!said.is_empty()).then(|| said.to_owned())
}

fn rows<'a>(document: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    document
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn column(document: &serde_json::Value, group: &str, name: &str) -> Option<u64> {
    document.get("accounting")?.get(group)?.get(name)?.as_u64()
}

fn sum(counts: &[Option<u64>]) -> Option<u64> {
    counts
        .iter()
        .try_fold(0_u64, |total, count| total.checked_add((*count)?))
}

fn size(count: usize) -> Option<u64> {
    match u64::try_from(count) {
        Ok(count) => Some(count),
        Err(_outside_wire_range) => None,
    }
}

fn plural(count: usize, thing: &str) -> String {
    if count == 1 {
        format!("{count} {thing}")
    } else {
        format!("{count} {thing}s")
    }
}
