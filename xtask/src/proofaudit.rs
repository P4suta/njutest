// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed run recorded.
//!
//! [ADR 0004](../../docs/adr/0004-proof-layers-not-budgets.md) ships a proof layer only against a re-implementation that never calls the runner's, so nothing here consults the code that wrote the report: every verdict is re-derived from the recording alone, and wherever the recording does not carry enough to re-derive one, that is said plainly rather than read as agreement.

mod knobs;
pub mod merge;
pub mod sentinel;
pub mod soundness;

use crate::error::Coded as _;
pub use crate::layers::Coverage;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use sha2::Digest as _;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "njutest-assurance-report-v1.json";

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

/// The exit code an audit that found no violation and left something unaudited earns, kept apart so a step that reads only the code cannot take it for an audit that checked everything.
pub const EXIT_UNAUDITED: u8 = 3;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const UNREACHED: &str = "unreached";
const REJECTED: &str = "compile-rejected";
const STEP_LIMIT_REACHED: &str = "step-limit-reached";
const WAITED: &str = "waited";
const UNCONFIRMED: &str = "unconfirmed";
const ERRORED: &str = "errored";
const DECLINED: &str = "declined";
const PASSED: &str = "passed";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNNOTICED_FAULT: &str = "unnoticed-fault";
const DIMENSION_NOT_MEASURED: &str = "dimension-not-measured";
const CORRUPT_AFTER_CRASH: &str = "corrupt-after-crash";
const BROKEN_UNDER_FAULT: &str = "broken-under-fault";
const NOT_MEASURED_FINDING: &str = "not-measured";
/// Every finding kind that is something wrong with the code under test, as `docs/report-v1.md` marks them, which is what lets a run conclude DEFECT.
pub const DEFECT_KINDS: [&str; 7] = [
    "build-failure",
    "failing-test",
    "undefined-behaviour",
    "broken-under-fault",
    "environment-dependent",
    "corrupt-after-crash",
    "schedule-dependent",
];
const WAITED_MUTANT: &str = "waited-mutant";
const STEP_LIMIT_REACHED_MUTANT: &str = "step-limit-reached-mutant";
const FAILING_TEST: &str = "failing-test";
const TARGET_MISSING: &str = "target-missing";
const NOT_MEASURED: &str = "not-measured";
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
    /// The report passed its schema and still lacks a field a layer reads, which the schema and the reader disagree about.
    #[error("{path}: the report has no field a layer reads: {cause}")]
    UnreadReport {
        /// The report.
        path: String,
        /// Which field.
        #[source]
        cause: crate::route::ReadCauseError,
    },
    /// A report given with its shards is not the merge of any.
    #[error("{path}: not a report merged from shards, so there are no shards to hold it to")]
    NotMerged {
        /// The document.
        path: String,
    },
    /// A document given as a shard is not one.
    #[error("{path}: given as a shard, and not a shard document")]
    NotAShard {
        /// The document.
        path: String,
    },
    /// One shard was given more than once.
    #[error("shard {run_id} was given more than once")]
    ShardGivenTwice {
        /// The run it names.
        run_id: String,
    },
    /// The document is on its published schema and not one this audit can read into the two documents that schema describes.
    #[error("{path}: on its schema and not a document this audit reads: {source}")]
    Unshaped {
        /// The document.
        path: String,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// A shard was given that the merged report does not name.
    #[error("{path}: shard {run_id} is not one the report was merged from")]
    ShardNotMerged {
        /// The shard document.
        path: String,
        /// The run it names.
        run_id: String,
    },
    /// The document is a complete report off its published schema, so a reader could meet an absent required field.
    #[error("{path}: off the published report schema: {source}")]
    OffSchema {
        /// The document.
        path: String,
        /// Where and how.
        #[source]
        source: crate::schemas::OffSchemaError,
    },
    /// The published report schema itself does not compile.
    #[error(transparent)]
    Schema(#[from] crate::schemas::SchemaError),
    /// The document is a whole report of a shape this audit does not project onto the one build it re-decides.
    #[error("{path}: {shape}; this audit re-decides one configured build measured whole")]
    Unprojected {
        /// The document.
        path: String,
        /// What it holds instead.
        shape: UnprojectableError,
    },
}

impl crate::error::Coded for AuditError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Unreadable { .. } => crate::error::XtCode::ProofUnreadable,
            Self::Unparsable { .. } => crate::error::XtCode::ProofUnparsable,
            Self::MalformedRecording { .. } => crate::error::XtCode::ProofRecording,
            Self::Unprojected { .. } => crate::error::XtCode::ProofUnprojected,
            Self::OffSchema { .. } => crate::error::XtCode::ProofOffSchema,
            Self::Schema(_) => crate::error::XtCode::SchemaUncompilable,
            Self::NotMerged { .. } => crate::error::XtCode::ProofNotMerged,
            Self::NotAShard { .. } => crate::error::XtCode::ProofNotAShard,
            Self::ShardGivenTwice { .. } => crate::error::XtCode::ProofShardTwice,
            Self::ShardNotMerged { .. } => crate::error::XtCode::ProofShardNotMerged,
            Self::Unshaped { .. } => crate::error::XtCode::ProofUnshaped,
            Self::UnreadReport { .. } => crate::error::XtCode::ProofUnreadReport,
        }
    }
}

/// What a report holds instead of the one configured build measured whole this audit re-decides.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UnprojectableError {
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
    /// The document is one part laid flat, which no run writes.
    #[error("a flat part with no `document_type`, which no run writes")]
    Flat,
}

impl crate::error::Coded for UnprojectableError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::ProofUnprojected
    }
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
    /// A merged report's parts, held to the shard documents it names.
    Merge,
    /// Which targets reached something different on a control than on their baseline, re-derived from the engine's touch records and held to what the report says of each.
    Drift,
    /// What each fault site came to, re-derived from the fault executions alone and held to the report's records, counts and findings.
    Faults,
    /// What each disposition run again against a moved target came to, re-derived from its own execution and touch record (ADR 0036).
    Repair,
    /// What each control started under a knob established, re-derived from the engine's perturbed-control records and held to what the report says of each.
    Knobs,
    /// What each call that writes came to under a crash, re-derived from the recorded runs alone and held to the report's records and findings.
    Crashes,
    /// Which dimensions a run that asks every one of them did not establish, re-derived from the records and held to the findings that name them.
    Dimensions,
    /// Which test binaries the report proves single-threaded, held to the reach their baseline recorded off their tests' threads.
    Concurrency,
    /// Each mutation's reported outcome, held to the executions of it the recording holds.
    Executions,
    /// What interpreting the suite established, re-derived from what the interpreter said.
    Soundness,
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
            Self::Merge => "merge",
            Self::Drift => "drift",
            Self::Faults => "faults",
            Self::Repair => "repair",
            Self::Knobs => "knobs",
            Self::Dimensions => "dimensions",
            Self::Crashes => "crashes",
            Self::Concurrency => "concurrency",
            Self::Executions => "executions",
            Self::Soundness => "soundness",
        }
    }
}

/// What a layer hands back to show it said how far it got, which only [`Notes::looked`] and [`Notes::absent`] make.
#[must_use]
#[derive(Debug)]
struct Decided(());

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
    /// How far each layer got.
    pub coverage: BTreeMap<Layer, Coverage>,
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
        if self.violations() > 0 {
            1
        } else if self.unaudited() > 0 {
            EXIT_UNAUDITED
        } else {
            0
        }
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
        for (layer, coverage) in &self.coverage {
            writeln!(f, "layer: {}: {coverage}", layer.label())?;
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

    /// The layer looked at everything the recording owes it, and its remarks say what it found.
    fn looked(self) -> Decided {
        let partly = self
            .audit
            .remarks
            .iter()
            .any(|remark| remark.layer == self.layer && remark.standing == Standing::Unaudited);
        let coverage = if partly {
            Coverage::Partly
        } else {
            Coverage::Rederived
        };
        self.audit.coverage.insert(self.layer, coverage);
        Decided(())
    }

    /// The recording holds nothing this layer re-decides, for the reason `why`.
    fn absent(self, why: &'static str) -> Decided {
        self.audit
            .coverage
            .insert(self.layer, Coverage::Absent(why));
        Decided(())
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

/// The report an audit re-decides: where it was read from, and what it says.
#[derive(Debug, Clone, Copy)]
pub struct Reported<'a> {
    /// Where it was read from, as a reader is told it.
    pub path: &'a str,
    /// What it says.
    pub text: &'a str,
}

/// What a run recorded beside its report: the runner's recording and every engine recording under it, each as its path and its text.
#[derive(Debug, Clone, Copy)]
pub struct Recorded<'a> {
    /// The runner's recording, when the run kept one.
    pub runner: Option<(&'a str, &'a str)>,
    /// Every configured build's engine recording, in namespace order.
    pub engines: &'a [(String, String)],
    /// What the recording kept of each run the audit re-derives from, by the path its exec record gives, held to that record's size and digest.
    pub outputs: &'a [(String, soundness::Kept)],
}

/// The runner's recording, where the run kept one, with the path it was read from.
type RunnerRecording<'a> = (&'a str, crate::route::Checked<crate::schemas::RunnerLines>);

/// What `read` makes of the runner's recording, where the run kept one.
fn read_runner<T>(
    runner: Option<&RunnerRecording<'_>>,
    read: impl Fn(
        &crate::route::Checked<crate::schemas::RunnerLines>,
    ) -> Result<T, crate::route::ReadError>,
) -> Result<Option<T>, AuditError> {
    runner
        .map(|(recording_path, checked)| {
            read(checked).map_err(|source| AuditError::MalformedRecording {
                path: (*recording_path).to_owned(),
                source,
            })
        })
        .transpose()
}

/// Re-decides a report against what the run recorded beside it and, when `run` is present, re-reads every retained model-checker artifact from that exact run directory.
///
/// # Errors
/// [`AuditError::Unparsable`] for a document that is not JSON, [`AuditError::OffSchema`] for a complete report off its published schema,
/// [`AuditError::Unprojected`] for a report this audit does not re-decide as one build, [`AuditError::MalformedRecording`] for a recording with a corrupt line, and the corresponding retained-artifact error when `run` cannot be re-read.
pub fn audit_with(
    checkers: &crate::schemas::Checkers,
    Reported { path, text }: Reported<'_>,
    recorded: Recorded<'_>,
    run: Option<&Path>,
) -> Result<Audit, AuditError> {
    let document = read_report(checkers, path, text)?;
    let mut recording = Recording::of(&document).map_err(|cause| AuditError::UnreadReport {
        path: path.to_owned(),
        cause,
    })?;
    let runner = recorded
        .runner
        .map(|(recording_path, text)| {
            crate::route::Checked::read(text, checkers)
                .map(|checked| (recording_path, checked))
                .map_err(|source| AuditError::MalformedRecording {
                    path: recording_path.to_owned(),
                    source,
                })
        })
        .transpose()?;
    recording.verdict = runner.as_ref().and_then(|(_, checked)| concluded(checked));
    let RunnerEvidence {
        routing,
        watched,
        faulted,
        crashed,
        repairs,
        recorded_executions,
    } = runner_evidence(runner.as_ref())?;
    let engines = engine_evidence(checkers, recorded.engines)?;
    let mut audit = Audit {
        run_id: recording.run_id.clone(),
        mutants: recording.mutants.len(),
        targets: recording.targets.len(),
        remarks: Vec::new(),
        coverage: BTreeMap::new(),
    };
    for layer in Layer::ALL {
        let Decided(()) = match layer {
            Layer::Accounting => accounting(&recording, &mut audit),
            Layer::Killers => killers(&recording, &mut audit),
            Layer::Findings => findings(&recording, &mut audit),
            Layer::Acceptances => acceptances(&recording, &mut audit),
            Layer::Reuse => reuse(&recording, routing.as_ref(), &mut audit),
            Layer::Proofs => proofs(&recording, routing.as_ref(), &repairs, &mut audit),
            Layer::Executions => executions(&recording, routing.as_ref(), &mut audit),
            Layer::Hollow => hollow(&recording, routing.as_ref(), &mut audit),
            Layer::Wire => wire(&recording, watched.as_ref(), &mut audit),
            Layer::Model => models(&recording, run, &mut audit),
            Layer::Merge => Notes::on(&mut audit, Layer::Merge)
                .absent("this report is one run's, and a merge is audited against its shards"),
            Layer::Drift => drift(
                &recording,
                (&engines, &repairs, routing.as_ref()),
                &mut audit,
            ),
            Layer::Repair => repaired(
                &recording,
                (&engines, &repairs, routing.as_ref()),
                &mut audit,
            ),
            Layer::Faults => faults(&recording, faulted.as_ref(), &mut audit),
            Layer::Dimensions => dimensions(&recording, &mut audit),
            Layer::Crashes => crashes(&recording, crashed.as_ref(), &mut audit),
            Layer::Knobs => knobs::audited(&recording, &engines, &mut audit),
            Layer::Concurrency => concurrency(&recording, &engines, &mut audit),
            Layer::Soundness => soundness::audited(
                &recording,
                recorded_executions.as_deref(),
                recorded.outputs,
                &mut audit,
            ),
        };
    }
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// What each engine recording says of touch and perturbed controls, read from its stream alone.
fn engine_evidence(
    checkers: &crate::schemas::Checkers,
    recorded: &[(String, String)],
) -> Result<Vec<Engine>, AuditError> {
    recorded
        .iter()
        .map(|(recording_path, text)| {
            let malformed = |source| AuditError::MalformedRecording {
                path: recording_path.clone(),
                source,
            };
            let checked = crate::route::Checked::read(text, checkers).map_err(malformed)?;
            Ok(Engine {
                touched: crate::drift::read(&checked),
                perturbed: crate::knobs::read(&checked),
            })
        })
        .collect()
}

/// What the runner's recording says of routing, seams, faults, crashes, repairs and executions, each read from the stream alone; nothing where the run kept none.
fn runner_evidence(runner: Option<&RunnerRecording<'_>>) -> Result<RunnerEvidence, AuditError> {
    Ok(RunnerEvidence {
        routing: read_runner(runner, crate::route::read)?,
        watched: read_runner(runner, crate::wire::read)?,
        faulted: runner.map(|(_, checked)| crate::faults::read(checked)),
        crashed: runner.map(|(_, checked)| crate::crashes::read(checked)),
        repairs: runner
            .map(|(_, checked)| crate::repair::read(checked))
            .unwrap_or_default(),
        recorded_executions: runner.map(|(_, checked)| executions_of(checked)),
    })
}

/// What the runner's recording says, read once.
struct RunnerEvidence {
    routing: Option<crate::route::Routing>,
    watched: Option<crate::wire::Watched>,
    faulted: Option<crate::faults::Faulted>,
    crashed: Option<crate::crashes::Crashed>,
    repairs: Vec<crate::repair::Repair>,
    recorded_executions: Option<Vec<serde_json::Value>>,
}

/// Every exec record a runner's recording holds, in the order it holds them.
fn executions_of(
    recorded: &crate::route::Checked<crate::schemas::RunnerLines>,
) -> Vec<serde_json::Value> {
    recorded
        .events()
        .iter()
        .filter(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("exec"))
        .filter_map(|event| event.get("exec").cloned())
        .collect()
}

/// The flat view of the report at `path` holding `text`, once it is JSON, on its published schema, and one build measured whole.
///
/// # Errors
/// [`AuditError::Unparsable`], [`AuditError::OffSchema`], [`AuditError::Schema`] or [`AuditError::Unprojected`], in that order.
fn read_report(
    checkers: &crate::schemas::Checkers,
    path: &str,
    text: &str,
) -> Result<serde_json::Value, AuditError> {
    let read: serde_json::Value =
        crate::strictjson::from_str(text).map_err(|source| AuditError::Unparsable {
            path: path.to_owned(),
            source,
        })?;
    if read.get("document_type").is_some() {
        checkers
            .assurance_report()
            .check(&read)
            .map_err(|source| AuditError::OffSchema {
                path: path.to_owned(),
                source,
            })?;
    }
    projected(&read).map_err(|shape| AuditError::Unprojected {
        path: path.to_owned(),
        shape,
    })
}

/// What the runner's recording says the run concluded, from its `run-end`; nothing where it holds none.
fn concluded(recorded: &crate::route::Checked<crate::schemas::RunnerLines>) -> Option<String> {
    recorded
        .events()
        .iter()
        .rev()
        .find(|event| field(event, "type").as_deref() == Some("run-end"))
        .and_then(|event| event.get("run"))
        .and_then(|run| field(run, "verdict"))
}

/// The flat view of the one part of one configured build that every layer re-decides: a complete report's one build measured whole, or a shard's one build with the shard it is written into its scope.
fn projected(document: &serde_json::Value) -> Result<serde_json::Value, UnprojectableError> {
    let Some(kind) = document.get("document_type") else {
        return Err(UnprojectableError::Flat);
    };
    let mut report = document.get("report").cloned().unwrap_or_default();
    let builds = rows(&report, "builds");
    let [build] = builds else {
        return Err(UnprojectableError::Builds {
            count: builds.len(),
        });
    };
    let part = if kind.as_str() == Some("shard") {
        build.get("source").cloned().unwrap_or_default()
    } else {
        let parts = rows(build, "parts");
        let [part] = parts else {
            return Err(UnprojectableError::Parts { count: parts.len() });
        };
        part.clone()
    };
    let owned = report.get("shard").and_then(|shard| {
        Some(format!(
            "{}/{}",
            shard.get("index")?.as_u64()?,
            shard.get("of")?.as_u64()?
        ))
    });
    if let (Some(owned), Some(scope)) = (
        owned,
        report
            .get_mut("scope")
            .and_then(serde_json::Value::as_object_mut),
    ) {
        scope.insert("shard".to_owned(), serde_json::Value::String(owned));
    }
    let part = &part;
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
        "beside",
        "knobs",
        "seams",
        "crashes",
        "concurrency",
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

/// Every binary the engine built that the report records nothing about, each a violation.
fn unrecorded(rows: &[serde_json::Value], touched: &crate::drift::Touched, notes: &mut Notes<'_>) {
    if touched.kinds.is_empty() {
        notes.unaudited(
            "concurrency",
            "the engine recording holds no build record, so which binaries a run measured, and \
             so which records the report owes, is not known"
                .to_owned(),
        );
    }
    let mut unverified = Vec::new();
    for target in touched.kinds.keys() {
        if !touched.verified.contains(target) {
            unverified.push(target.as_str());
            continue;
        }
        if touched.passing.contains(target)
            && !rows
                .iter()
                .any(|row| field(row, "target").as_deref() == Some(target.as_str()))
        {
            notes.violated(
                target,
                format!(
                    "the engine built {target} and its baseline passed, and the report, which \
                     measured mutants, records nothing about its threads, so it is neither proven \
                     nor named as a hole"
                ),
            );
        }
    }
    if !unverified.is_empty() {
        notes.unaudited(
            "concurrency",
            format!(
                "the engine built {} and recorded no baseline of them, so whether they were skipped \
                 by name or their record is missing, and so whether the report owes a thread \
                 record for them, is not known",
                unverified.join(", ")
            ),
        );
    }
}

/// What a thread record the report holds that names no target, or no standing or exploration, is.
const UNSHAPED_THREADS: &str = "a thread record of the report is not the shape a run writes it in";

/// Each test binary the report calls single-threaded, or concurrent for reach off its tests' threads, held to what the engine's baseline touch record says it reached there.
///
/// The source half of the proof is a scan of every package the binary links, which this audit does not repeat, so it is said to be unaudited rather than read as agreement.
fn concurrency(recording: &Recording<'_>, engines: &[Engine], audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Concurrency);
    let rows = rows(recording.document, "concurrency");
    let executed = recording
        .document
        .pointer("/accounting/mutants/executed")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|executed| executed > 0);
    if rows.is_empty() && !executed {
        return notes.absent(
            "the report measured no mutant and records no binary's threads, so there is no standing to hold",
        );
    }
    let touched = match engines {
        [one] => &one.touched,
        [] | [_, _, ..] => {
            notes.unaudited(
                "concurrency",
                format!(
                    "the recording holds {} engine recordings where one build's baseline reach \
                     is what a single-threaded proof is held to",
                    engines.len()
                ),
            );
            return notes.looked();
        }
    };
    unrecorded(rows, touched, &mut notes);
    let mut proven: Vec<String> = Vec::new();
    for row in rows {
        let Some((target, standing)) = field(row, "target").zip(row.get("standing")) else {
            notes.violated("concurrency", UNSHAPED_THREADS.to_owned());
            continue;
        };
        let witnessed = crate::concurrency::Witnessed {
            loose: touched
                .touches
                .iter()
                .rev()
                .find(|touch| {
                    touch.measured == crate::drift::Measured::Baseline && touch.target == target
                })
                .map(|touch| touch.loose),
            kind: touched.kinds.get(&target).cloned(),
            args: touched.args.get(&target).cloned(),
        };
        let derived = match crate::concurrency::derived(&witnessed) {
            Ok(derived) => derived,
            Err(why) => {
                notes.violated(
                    &target,
                    format!(
                        "the report gives {target} a standing the recording cannot rest: {}",
                        why.coded()
                    ),
                );
                continue;
            }
        };
        match crate::concurrency::agrees(standing, &derived) {
            Ok(()) => {
                if field(standing, "state").as_deref() == Some("single-threaded") {
                    proven.push(target.clone());
                }
            }
            Err(why) => notes.violated(&target, format!("{target}: {}", why.coded())),
        }
    }
    explorations(rows, engines, &mut notes);
    if !proven.is_empty() {
        notes.unaudited(
            "concurrency",
            format!(
                "{} test binary(ies) are proven single-threaded partly by a scan of every \
                 package they link, which this audit does not repeat",
                proven.len()
            ),
        );
    }
    notes.looked()
}

fn engine_of(engines: &[Engine]) -> Vec<&crate::knobs::Perturbed> {
    match engines {
        [one] => one
            .perturbed
            .controls
            .iter()
            .filter(|control| match control.started.role() {
                crate::knobs::Role::Delayed | crate::knobs::Role::Undelayed => true,
                crate::knobs::Role::Knob(_) | crate::knobs::Role::Unknown => false,
            })
            .collect(),
        [] | [_, _, ..] => Vec::new(),
    }
}

/// What each binary's exploration came to, replayed from the controls the engine started for it in the order it recorded them, held to the report exactly, and why one with none was not explored, derived from its baseline (ADR 0034).
fn explorations(rows: &[serde_json::Value], engines: &[Engine], notes: &mut Notes<'_>) {
    let [engine] = engines else {
        return;
    };
    let explored = engine_of(engines);
    let touched = &engine.touched;
    for row in rows {
        let Some((target, reported)) = field(row, "target").zip(row.get("explored")) else {
            notes.violated("concurrency", UNSHAPED_THREADS.to_owned());
            continue;
        };
        let runs: Vec<crate::concurrency::Run> = explored
            .iter()
            .filter(|control| control.target == target)
            .map(|control| crate::concurrency::Run {
                delayed: control.started.delayed,
                confirms: control.started.confirms,
                ended: control.ended,
                failed: control.failed.clone(),
            })
            .collect();
        let context = crate::concurrency::Context {
            single_threaded: row
                .get("standing")
                .and_then(|standing| field(standing, "state"))
                .as_deref()
                == Some("single-threaded"),
            passing: touched.passing.contains(&target),
            reached: touched
                .touches
                .iter()
                .rev()
                .find(|touch| {
                    touch.measured == crate::drift::Measured::Baseline && touch.target == target
                })
                .map(|touch| touch.reached.len()),
            asked_any: !explored.is_empty(),
        };
        match crate::concurrency::replayed(&runs) {
            Ok(derived) => {
                if let Err(why) = crate::concurrency::agrees_explored(reported, &derived, context) {
                    notes.violated(&target, format!("{target}: {}", why.coded()));
                }
            }
            Err(why) => notes.violated(
                &target,
                format!(
                    "{target}: the controls the engine recorded for it are not an exploration: {}",
                    why.coded()
                ),
            ),
        }
    }
}

/// The finding a report raises about a target whose baseline reach moved.
const UNSTABLE_BASELINE: &str = "unstable-baseline";
const REACH_MOVED: &str = "reach-moved";

/// The limitation a report states about the targets no comparable control measured.
const DRIFT_NOT_MEASURED: &str = "drift-not-measured";

/// Which targets moved between their baseline and a control, re-derived from the engine's touch records and held to the report's records, findings, and limitation.
/// What one engine recording holds, as the layers that re-derive from it read it.
#[derive(Debug)]
struct Engine {
    /// Every touch record.
    touched: crate::drift::Touched,
    /// Every perturbed control.
    perturbed: crate::knobs::Perturbations,
}

/// Each target and state the report's drift records name; nothing where the report records no drift.
fn recorded_drift(recording: &Recording<'_>) -> Option<Vec<(String, String)>> {
    recording.document.get("drift").map(|rows| {
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
            .collect()
    })
}

fn drift(
    recording: &Recording<'_>,
    (engines, repairs, routing): (
        &[Engine],
        &[crate::repair::Repair],
        Option<&crate::route::Routing>,
    ),
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Drift);
    let recorded = recorded_drift(recording);
    let touched = match (engines, recorded.as_ref()) {
        ([], None) => {
            return notes.absent(
                "the report records no drift and the run kept no engine recording to derive one from",
            );
        }
        ([], Some(_)) => {
            notes.unaudited(
                "drift",
                "the run kept no engine recording, so which targets moved between their \
                 baseline and a control cannot be re-derived"
                    .to_owned(),
            );
            return notes.looked();
        }
        ([one], _) => &one.touched,
        (several, _) => {
            notes.unaudited(
                "drift",
                format!(
                    "the recording holds {} engine recordings and the report is one build's, \
                     so which of them it answers to cannot be told from the recording",
                    several.len()
                ),
            );
            return notes.looked();
        }
    };
    if touched.unreadable > 0 {
        notes.unaudited(
            "drift",
            format!(
                "{} touch record(s) do not say which run they were measured on, which mutation \
                 a repair ran, or what it reached, so what they would have shown cannot be \
                 counted as agreement",
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
        return notes.looked();
    };
    held_to_records(&derived, &recorded, &mut notes);
    if recording.shard.is_some() {
        notes.unaudited(
            "drift",
            "this shard does not hold the whole catalog, so which moved targets are owed \
             unstable-baseline and which unmeasured ones drift-not-measured is decided over the \
             combined parts when they are merged"
                .to_owned(),
        );
        return notes.looked();
    }
    let resting = resting(recording, &derived, (repairs, routing));
    held_to_findings(recording, &resting, &mut notes);
    held_to_limitation(recording, &derived, &mut notes);
    held_to_repairs(recording, &resting, &mut notes);
    notes.looked()
}

/// How many dispositions still rest on each moved target: a survivor or an unreached claim whose route did not put the target to it, and which no repair against it that reached the site decided again; every one where the run kept no routing to tell.
fn resting(
    recording: &Recording<'_>,
    derived: &BTreeMap<String, crate::drift::Standing>,
    (repairs, routing): (&[crate::repair::Repair], Option<&crate::route::Routing>),
) -> BTreeMap<String, Option<usize>> {
    derived
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| {
            let count = routing.map(|routing| {
                recording
                    .mutants
                    .iter()
                    .filter(|row| row.outcome == SURVIVED || row.outcome == UNREACHED)
                    .filter(|row| {
                        !routing.routes.iter().any(|route| {
                            route.names(&row.id, &row.display_id)
                                && route.reaching.iter().any(|one| one == target)
                        })
                    })
                    .filter(|row| {
                        !repairs.iter().any(|repair| {
                            repair.mutant == row.display_id
                                && repair.target == *target
                                && repair.reached == "reached"
                        })
                    })
                    .count()
            });
            (target.clone(), count)
        })
        .collect()
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

/// The `reach-moved` limitation, owed exactly for each moved target nothing rests on any more, naming it.
fn held_to_repairs(
    recording: &Recording<'_>,
    resting: &BTreeMap<String, Option<usize>>,
    notes: &mut Notes<'_>,
) {
    let stated: Vec<String> = rows(recording.document, "limitations")
        .iter()
        .filter(|row| field(row, "name").as_deref() == Some(REACH_MOVED))
        .map(|row| field(row, "detail").unwrap_or_default())
        .collect();
    for (target, count) in resting {
        let named = stated.iter().any(|detail| listed(detail, target));
        match (count, named) {
            (Some(0), false) => notes.violated(
                target,
                format!(
                    "the reach of {target} moved and every disposition resting on it was decided \
                     again, and the report does not say {REACH_MOVED} about it"
                ),
            ),
            (Some(0), true) | (None | Some(_), false) => {}
            (None | Some(_), true) => notes.violated(
                target,
                format!(
                    "the report says {REACH_MOVED} about {target}, and something still rests on it"
                ),
            ),
        }
    }
}

/// What each disposition run again against a moved target came to, re-derived from its own execution and touch record and held to the repair record and the report (ADR 0036).
fn repaired(
    recording: &Recording<'_>,
    (engines, repairs, routing): (
        &[Engine],
        &[crate::repair::Repair],
        Option<&crate::route::Routing>,
    ),
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Repair);
    if repairs.is_empty() {
        return notes.absent("the run ran no disposition again against a target whose reach moved");
    }
    let (Some(routing), [Engine { touched, .. }]) = (routing, engines) else {
        notes.unaudited(
            "repair",
            format!(
                "the report rests on {} repair(s), and the recording holds no routing or not \
                 exactly one engine recording to re-derive them from",
                repairs.len()
            ),
        );
        return notes.looked();
    };
    let moved = crate::drift::standings(touched);
    let paired = paired(recording, repairs, (routing, touched), &mut notes);
    for (at, repair) in repairs.iter().enumerate() {
        let before = repairs
            .get(..at)
            .and_then(|earlier| earlier.iter().rev().find(|one| one.mutant == repair.mutant))
            .map(|earlier| earlier.now.as_str());
        one_repair(
            recording,
            (repair, before, &moved, routing),
            &paired,
            &mut notes,
        );
    }
    for row in &recording.mutants {
        if let Some(last) = repairs
            .iter()
            .rev()
            .find(|one| one.mutant == row.display_id)
            && row.outcome.replace('_', "-") != last.now
        {
            notes.violated(
                &row.display_id,
                format!(
                    "the last repair of {} made it {}, and the report says {}",
                    row.display_id, last.now, row.outcome
                ),
            );
        }
    }
    notes.looked()
}

/// Each repair's last execution against its moved target, paired with the one engine repair touch record naming that mutation and target; a repair touch no repair names, or a repair two of them name, is a violation.
fn paired<'a>(
    recording: &Recording<'_>,
    repairs: &[crate::repair::Repair],
    (routing, touched): (&'a crate::route::Routing, &'a crate::drift::Touched),
    notes: &mut Notes<'_>,
) -> Vec<(&'a crate::route::Exec, Option<&'a crate::drift::Touch>)> {
    let full = |display: &str| {
        recording
            .mutants
            .iter()
            .find(|row| row.display_id == display)
            .map(|row| row.id.clone())
    };
    let repair_touches: Vec<&crate::drift::Touch> = touched
        .touches
        .iter()
        .filter(|touch| touch.measured == crate::drift::Measured::Repair)
        .collect();
    for touch in &repair_touches {
        let claimed = repairs.iter().any(|repair| {
            repair.target == touch.target
                && full(&repair.mutant).is_none_or(|id| touch.mutant.as_ref() == Some(&id))
        });
        if !claimed {
            notes.violated(
                &touch.target,
                format!(
                    "the engine recorded a repair touch of {} against {}, and no repair record \
                     says that mutation was run again there",
                    match touch.mutant.as_deref() {
                        Some(mutant) => mutant,
                        None => "a mutation it did not name",
                    },
                    touch.target
                ),
            );
        }
    }
    let mut pairs = Vec::new();
    for repair in repairs {
        let id = full(&repair.mutant);
        let Some(exec) = routing.execs.iter().rev().find(|exec| {
            exec.target == repair.target
                && (exec.mutant == repair.mutant || id.as_ref() == Some(&exec.mutant))
        }) else {
            continue;
        };
        let naming: Vec<&crate::drift::Touch> = repair_touches
            .iter()
            .copied()
            .filter(|touch| {
                touch.target == repair.target && touch.mutant.is_some() && touch.mutant == id
            })
            .collect();
        match naming.as_slice() {
            [] => pairs.push((exec, None)),
            [one] => pairs.push((exec, Some(*one))),
            several => notes.violated(
                &repair.mutant,
                format!(
                    "{} repair touch records name it against {}, so which one its repair \
                     reached through cannot be told",
                    several.len(),
                    repair.target
                ),
            ),
        }
    }
    pairs
}

/// Whether a repair's disposition rested on its target: the target moved, the route did not put it, and what it was is what the last earlier repair of it made it, or its route where none did.
fn rested(
    (repair, before): (&crate::repair::Repair, Option<&str>),
    (moved, routing): (
        &BTreeMap<String, crate::drift::Standing>,
        &crate::route::Routing,
    ),
    notes: &mut Notes<'_>,
) {
    let subject = &repair.mutant;
    if moved.get(&repair.target) != Some(&crate::drift::Standing::Moved) {
        notes.violated(
            subject,
            format!(
                "it was run again against {}, whose reach the touch records do not show moving",
                repair.target
            ),
        );
    }
    let route = routing.routes.iter().find(|route| route.mutant == *subject);
    if route.is_some_and(|route| route.reaching.contains(&repair.target)) {
        notes.violated(
            subject,
            format!(
                "its route already put {} to it, so nothing of it rested on that target",
                repair.target
            ),
        );
    }
    let (expected_was, by) = match before {
        Some(now) => (now, "the repair of it before this one made it"),
        None if route.is_some_and(|route| route.granularity == UNREACHED) => {
            (UNREACHED, "its route makes it")
        }
        None => (SURVIVED, "its route makes it"),
    };
    if repair.was != expected_was {
        notes.violated(
            subject,
            format!(
                "the repair says it was {}, and {by} {expected_was}",
                repair.was
            ),
        );
    }
}

/// One repair held to what its target, route, last execution and touch record decide.
fn one_repair(
    recording: &Recording<'_>,
    (repair, before, moved, routing): (
        &crate::repair::Repair,
        Option<&str>,
        &BTreeMap<String, crate::drift::Standing>,
        &crate::route::Routing,
    ),
    paired: &[(&crate::route::Exec, Option<&crate::drift::Touch>)],
    notes: &mut Notes<'_>,
) {
    let subject = &repair.mutant;
    rested((repair, before), (moved, routing), notes);
    let Some((last, touch)) = paired
        .iter()
        .rev()
        .find(|(exec, _)| exec.mutant == *subject && exec.target == repair.target)
    else {
        notes.violated(
            subject,
            crate::repair::RepairContradictionError::NotRun {
                target: repair.target.clone(),
            }
            .to_string(),
        );
        return;
    };
    let Some(index) = recording
        .mutants
        .iter()
        .find(|row| row.display_id == *subject)
        .and_then(|row| row.catalog_index)
    else {
        notes.unaudited(
            subject,
            "the report names no catalog index for it, so whether its repair reached its site \
             cannot be read from the touch record"
                .to_owned(),
        );
        return;
    };
    match crate::repair::derived(&repair.was, &last.outcome, (index, *touch)) {
        Ok(derived) => {
            if repair.reached != derived.reached {
                notes.violated(
                    subject,
                    format!(
                        "the repair says its run {} the site, and its touch record says {}",
                        repair.reached, derived.reached
                    ),
                );
            }
            if !derived.now.contains(&repair.now) {
                notes.violated(
                    subject,
                    format!(
                        "the repair made it {}, and its last execution against {} decides {}",
                        repair.now,
                        repair.target,
                        derived.now.join(" or ")
                    ),
                );
            }
        }
        Err(why) => notes.violated(subject, why.coded()),
    }
}

fn held_to_findings(
    recording: &Recording<'_>,
    resting: &BTreeMap<String, Option<usize>>,
    notes: &mut Notes<'_>,
) {
    let owed: BTreeSet<&str> = resting
        .iter()
        .filter(|(_, count)| count.is_none_or(|count| count > 0))
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
                 baseline did not, something still rests on it, and the report raises no \
                 {UNSTABLE_BASELINE} finding about it"
            ),
        );
    }
    for target in named.difference(&owed) {
        notes.violated(
            target,
            format!(
                "the report raises {UNSTABLE_BASELINE} about {target}, and either the engine's \
                 touch records do not show its reach moving or nothing rests on it any more"
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

/// Whether a limitation's detail names `target` in its closing list.
fn listed(detail: &str, target: &str) -> bool {
    named(detail).contains(&target)
}

/// The targets a limitation's detail names in its closing list, which is how a report names the targets a limitation is about.
fn named(detail: &str) -> Vec<&str> {
    detail
        .rsplit_once(" (")
        .and_then(|(_, list)| list.strip_suffix(')'))
        .map(|list| list.split(", ").collect())
        .unwrap_or_default()
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
    catalog_index: Option<u64>,
    outcome: String,
    acceptance: AcceptanceFact,
    killed_by: Option<String>,
    read_back_from: Option<String>,
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

fn models(recording: &Recording<'_>, run: Option<&Path>, audit: &mut Audit) -> Decided {
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
    notes.looked()
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

impl crate::error::Coded for MintMutantIdError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::FieldTooLong { .. } => crate::error::XtCode::IdentityField,
        }
    }
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
fn hollow(
    recording: &Recording<'_>,
    routing: Option<&crate::route::Routing>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Hollow);
    if recording.shard.is_some() {
        notes.unaudited(
            "hollow",
            "this shard does not hold the whole catalog, so which targets answered about a \
             mutation and noticed none is decided over the combined parts when they are merged"
                .to_owned(),
        );
        return notes.looked();
    }
    let Some(routing) = routing else {
        notes.unaudited(
            "executions",
            "the run kept no recording of what it ran, so which targets were put to a \
             mutation and noticed none cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    if routing.execs.is_empty() {
        notes.unaudited(
            "executions",
            "the recording holds no mutation execution, so no target was put to anything \
             this audit could hold it to"
                .to_owned(),
        );
        return notes.looked();
    }
    let asked = match asked_targets(routing) {
        Ok(asked) => asked,
        Err(overflow) => {
            notes.violated(
                overflow.target,
                "the execution count exceeds the report wire's u64 range".to_owned(),
            );
            return notes.looked();
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
    notes.looked()
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
fn wire(
    recording: &Recording<'_>,
    watched: Option<&crate::wire::Watched>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Wire);
    let Some(watched) = watched else {
        notes.unaudited(
            "exchanges",
            "the run kept no recording of what it ran, so whether any exchange went past a seam \
             cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    if watched.exchanges.is_empty() && watched.execs.is_empty() {
        return notes.absent("no exchange went past a seam and no fault was put");
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
        return notes.looked();
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
    notes.looked()
}

/// What each fault site came to, re-derived from the recording's fault executions and held to the report (ADR 0032).
fn faults(
    recording: &Recording<'_>,
    faulted: Option<&crate::faults::Faulted>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Faults);
    let mut reported: Vec<crate::faults::Site> = Vec::new();
    for row in rows(recording.document, "faults") {
        match crate::faults::site(row) {
            Some(site) => reported.push(site),
            None => notes.violated(
                "faults",
                "a fault site of the report is not the shape a run writes it in".to_owned(),
            ),
        }
    }
    if let Some(unread) = faulted.filter(|faulted| !faulted.unread.is_empty()) {
        notes.unaudited(
            "faults",
            format!(
                "the recording holds {} fault record(s) without a field their schema requires ({}), \
                 which nothing is held to",
                unread.unread.len(),
                unread.unread.join(", ")
            ),
        );
    }
    fault_counts(recording, &reported, &mut notes);
    fault_findings(recording, &reported, &mut notes);
    besides(recording, &reported, faulted, &mut notes);
    let Some(faulted) = faulted else {
        if reported.is_empty() {
            return notes.looked();
        }
        notes.unaudited(
            "faults",
            format!(
                "the report holds {} fault site(s) and there is no recording to re-derive them from",
                reported.len()
            ),
        );
        return notes.looked();
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
            notes.violated(&site.fault, why.coded());
        }
    }
    notes.looked()
}

/// What a crash site the report holds that is not the shape a run writes is.
const UNSHAPED_CRASH: &str = "a crash site of the report is not the shape a run writes it in";

/// What each call that writes came to, re-derived from the recording's crash steps and held to the report exactly, with its counts and its findings in both directions (ADR 0035).
fn crashes(
    recording: &Recording<'_>,
    crashed: Option<&crate::crashes::Crashed>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Crashes);
    let mut reported: Vec<crate::crashes::Site> = Vec::new();
    for row in rows(recording.document, "crashes") {
        match crate::crashes::site(row) {
            Some(site) => reported.push(site),
            None => notes.violated("crashes", UNSHAPED_CRASH.to_owned()),
        }
    }
    crash_findings(recording, &reported, &mut notes);
    crash_accounting(recording.document, &reported, &mut notes);
    let Some(crashed) = crashed else {
        let no_site = rows(recording.document, "limitations")
            .iter()
            .any(|row| field(row, "name").as_deref() == Some("crash-no-site"));
        if !reported.is_empty() || no_site {
            notes.unaudited(
                "crashes",
                format!(
                    "the report holds {} crash site(s){} and there is no recording to re-derive \
                     what it put from",
                    reported.len(),
                    if no_site {
                        " and says there was none to put"
                    } else {
                        ""
                    }
                ),
            );
        }
        return notes.looked();
    };
    for (crash, why) in crate::crashes::disagreements(&reported, crashed) {
        notes.violated(&crash, why);
    }
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    for row in rows(recording.document, "crashes") {
        match field(row, "display_id").zip(field(row, "id")) {
            Some((display, id)) => {
                ids.insert(display, id);
            }
            None => notes.violated("crashes", UNSHAPED_CRASH.to_owned()),
        }
    }
    for (crash, why) in crate::crashes::issued_disagreements(&ids, crashed) {
        notes.violated(&crash, why);
    }
    notes.looked()
}

/// Each corrupt crash held to its `corrupt-after-crash` finding, and each unshared or undecided one to a `not-measured` finding, both ways.
fn crash_findings(
    recording: &Recording<'_>,
    reported: &[crate::crashes::Site],
    notes: &mut Notes<'_>,
) {
    let decided = |decisions: &[&str]| -> BTreeSet<&str> {
        reported
            .iter()
            .filter(|site| decisions.contains(&site.decision.as_str()))
            .map(|site| site.crash.as_str())
            .collect()
    };
    let crashes: BTreeSet<&str> = reported.iter().map(|site| site.crash.as_str()).collect();
    let named = |kind: &str| -> BTreeSet<&str> {
        recording
            .findings
            .iter()
            .filter(|finding| finding.kind == kind && crashes.contains(finding.subject.as_str()))
            .map(|finding| finding.subject.as_str())
            .collect()
    };
    for crash in decided(&["corrupt"]).symmetric_difference(&named(CORRUPT_AFTER_CRASH)) {
        notes.violated(
            crash,
            "the report's corrupt crashes and its corrupt-after-crash findings are not the same"
                .to_owned(),
        );
    }
    for crash in
        decided(&["unshared", "undecided"]).symmetric_difference(&named(NOT_MEASURED_FINDING))
    {
        notes.violated(
            crash,
            "the report's unshared and undecided crashes and its not-measured findings about \
             crashes are not the same"
                .to_owned(),
        );
    }
    let stray: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|finding| finding.kind == CORRUPT_AFTER_CRASH)
        .map(|finding| finding.subject.as_str())
        .filter(|subject| !crashes.contains(subject))
        .collect();
    for crash in stray {
        notes.violated(
            crash,
            "a corrupt-after-crash finding names no crash site of the report".to_owned(),
        );
    }
}

/// The report's crash counts held to what its sites add up to.
fn crash_accounting(
    document: &serde_json::Value,
    reported: &[crate::crashes::Site],
    notes: &mut Notes<'_>,
) {
    let counted = document
        .get("accounting")
        .and_then(|accounting| accounting.get("crashes"));
    if counted.is_none() && reported.is_empty() {
        return;
    }
    let count = |field: &str| {
        counted
            .and_then(|crashes| crashes.get(field))
            .and_then(serde_json::Value::as_u64)
    };
    let held = |decision: &str| {
        u64::try_from(
            reported
                .iter()
                .filter(|site| decision.is_empty() || site.decision == decision)
                .count(),
        )
    };
    for (field, decision) in [
        ("sites", ""),
        ("restarted", "restarted"),
        ("corrupt", "corrupt"),
        ("unshared", "unshared"),
        ("unreached", "unreached"),
        ("undecided", "undecided"),
        ("not_put", "not-put"),
    ] {
        let holds = held(decision);
        let agrees = match (&holds, count(field)) {
            (Ok(held), Some(counted)) => *held == counted,
            (Ok(_) | Err(_), None) | (Err(_), Some(_)) => false,
        };
        if !agrees {
            notes.violated(
                "crashes",
                format!(
                    "the report counts {:?} crash(es) as {field} and holds {holds:?}",
                    count(field)
                ),
            );
        }
    }
}

/// The dimensions a `whole-v1` run did not establish, re-derived from the flat part's records and held to its `dimension-not-measured` findings in both directions (ADR 0033).
fn dimensions(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Dimensions);
    let named: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|finding| finding.kind == DIMENSION_NOT_MEASURED)
        .map(|finding| finding.subject.as_str())
        .collect();
    if recording.contract != "whole-v1" || recording.shard.is_some() {
        for dimension in &named {
            notes.violated(
                dimension,
                "a run that does not ask every dimension, or a shard, raises no finding about one"
                    .to_owned(),
            );
        }
        return notes.looked();
    }
    let holed = holed_dimensions(recording);
    for dimension in holed.difference(&named) {
        notes.violated(
            dimension,
            "the records leave this dimension a hole and no finding says so".to_owned(),
        );
    }
    for dimension in named.difference(&holed) {
        notes.violated(
            dimension,
            "a finding says this dimension is a hole and the records establish it".to_owned(),
        );
    }
    notes.looked()
}

/// Whether some test binary's schedules were neither shown to need none nor broken by a delay, or a target passed with no record of its threads at all.
fn schedules_holed(document: &serde_json::Value) -> bool {
    let records = rows(document, "concurrency");
    let unrecorded = rows(document, "targets").iter().any(|target| {
        field(target, "status").as_deref() == Some("passed")
            && !records
                .iter()
                .any(|record| field(record, "target") == field(target, "name"))
    });
    unrecorded
        || records.iter().any(|record| {
            let explored = record.get("explored");
            let state = explored
                .and_then(|one| one.get("state"))
                .and_then(serde_json::Value::as_str);
            let why = explored
                .and_then(|one| one.get("why"))
                .and_then(serde_json::Value::as_str);
            !matches!(
                (state, why),
                (Some("broke"), _) | (Some("unexplored"), Some("not-needed"))
            )
        })
}

/// Whether the knobs, whose standings are `knobs`, leave repeatability open: none put, one left undecided, or one this machine lacked and another could put.
fn repeatable_holed(document: &serde_json::Value, knobs: &[String]) -> bool {
    let lacked = rows(document, "knobs").iter().any(|record| {
        record
            .get("standing")
            .and_then(|standing| standing.get("why"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|why| {
                [
                    "platform",
                    "zone-missing",
                    "locale-missing",
                    "shell-missing",
                ]
                .contains(&why)
            })
    });
    knobs.is_empty()
        || lacked
        || none_but_not_put(knobs)
        || knobs
            .iter()
            .any(|one| one == "uncompared" || one == "unsettled")
}

/// Whether a dimension put records and every one of them is one it could not put, which measures nothing.
fn none_but_not_put(decided: &[String]) -> bool {
    !decided.is_empty() && decided.iter().all(|one| one == "not-put")
}

/// Every dimension the flat part's records leave a hole, by name, read without any of the runner's code.
fn holed_dimensions(recording: &Recording<'_>) -> BTreeSet<&'static str> {
    let document = recording.document;
    let state = |key: &str, field: &str| -> Vec<String> {
        rows(document, key)
            .iter()
            .filter_map(|row| row.get(field))
            .filter_map(|value| {
                value
                    .get("state")
                    .or_else(|| value.get("decision"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect()
    };
    let limited = |name: &str| {
        rows(document, "limitations")
            .iter()
            .any(|row| field(row, "name").as_deref() == Some(name))
    };
    let mut holed = BTreeSet::new();
    if schedules_holed(document) {
        holed.insert("schedule");
    }
    if recording.mutants.iter().any(|mutant| {
        [
            "waited",
            "step-limit-reached",
            "unconfirmed",
            "errored",
            "declined",
        ]
        .contains(&mutant.outcome.as_str())
    }) {
        holed.insert("mutation");
    }
    if repeatable_holed(document, &state("knobs", "standing")) {
        holed.insert("repeatable");
    }
    let faults = state("faults", "decision");
    let unmeasured = recording
        .findings
        .iter()
        .any(|finding| finding.subject == "fault-baseline-not-measured");
    if unmeasured
        || (faults.is_empty() && !limited("fault-no-site"))
        || none_but_not_put(&faults)
        || faults
            .iter()
            .any(|one| one == "waited" || one == "undecided")
    {
        holed.insert("fault");
    }
    let crashes = state("crashes", "decision");
    let crashed_unmeasured = recording
        .findings
        .iter()
        .any(|finding| finding.subject == "crash-baseline-not-measured");
    if crashed_unmeasured
        || (crashes.is_empty() && !limited("crash-no-site"))
        || none_but_not_put(&crashes)
        || crashes
            .iter()
            .any(|one| one == "unshared" || one == "undecided")
    {
        holed.insert("durable");
    }
    let seams = state("seams", "answer");
    if limited("seam-not-watched") || seams.iter().any(|one| one == "unreached") {
        holed.insert("wire");
    }
    holed
}

/// Every survivor's evidence beside a fault the report holds, each that is not the shape a run writes a violation.
fn besides_of(recording: &Recording<'_>, notes: &mut Notes<'_>) -> Vec<crate::faults::Beside> {
    let mut held: Vec<crate::faults::Beside> = Vec::new();
    for row in rows(recording.document, "beside") {
        match crate::faults::beside(row) {
            Some(one) => held.push(one),
            None => notes.violated(
                "beside",
                "a survivor's evidence beside a fault is not the shape a run writes it in"
                    .to_owned(),
            ),
        }
    }
    held
}

/// The evidence a report holds beside faults, held to survivors and faults it holds and to what the recording says was told apart, in both directions.
fn besides(
    recording: &Recording<'_>,
    reported: &[crate::faults::Site],
    faulted: Option<&crate::faults::Faulted>,
    notes: &mut Notes<'_>,
) {
    let mut held = besides_of(recording, notes);
    held.sort();
    for one in &held {
        let survivor = recording
            .mutants
            .iter()
            .any(|mutant| mutant.display_id == one.mutant && mutant.outcome == "survived");
        let put = reported
            .iter()
            .any(|site| site.fault == one.fault && site.decision != "not-put");
        if !survivor || !put || !["beside", "alone"].contains(&one.failed.as_str()) {
            notes.violated(
                &one.mutant,
                format!(
                    "evidence beside {} names what is not a survivor beside a fault that was put, \
                     or no run that failed",
                    one.fault
                ),
            );
        }
    }
    let Some(faulted) = faulted else {
        return;
    };
    let mut asked: BTreeSet<(&str, &str)> = faulted
        .pairs
        .iter()
        .map(|pair| (pair.mutant.as_str(), pair.fault.as_str()))
        .collect();
    asked.extend(
        held.iter()
            .map(|one| (one.mutant.as_str(), one.fault.as_str())),
    );
    for (mutant, fault) in asked {
        let pairs: Vec<&crate::faults::Pair> = faulted
            .pairs
            .iter()
            .filter(|pair| pair.mutant == mutant && pair.fault == fault)
            .collect();
        let derived = crate::faults::derived(&pairs);
        let claimed = held
            .iter()
            .find(|one| one.mutant == mutant && one.fault == fault)
            .map(|one| (one.target.clone(), one.failed.as_str()));
        if derived
            .as_ref()
            .map(|(target, failed)| (target.clone(), *failed))
            != claimed
        {
            notes.violated(
                mutant,
                format!(
                    "the pairs of runs the recording holds beside {fault} support {derived:?}, \
                     and the report says {claimed:?}"
                ),
            );
        }
    }
    let mut recorded = faulted.besides.clone();
    recorded.sort();
    if held != recorded {
        notes.violated(
            "beside",
            format!(
                "the report holds {} piece(s) of evidence beside a fault and the recording {}, \
                 and they are not the same",
                held.len(),
                recorded.len()
            ),
        );
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

/// What the equivalence layer records when the compiler renders a mutation identically.
const IDENTICAL: &str = "identical";

/// What the engine calls a step-limit outcome in the executions it records.
const STEP_LIMIT_EXEC: &str = "step_limit_reached";

/// Every mutation's reported outcome against the executions of it the recording holds, so a report cannot say a test noticed what every recorded execution says survived.
fn executions(
    recording: &Recording<'_>,
    routing: Option<&crate::route::Routing>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Executions);
    let Some(routing) = routing else {
        notes.unaudited(
            "mutant-exec",
            "the run kept no recording of its executions, so no outcome can be held to what \
             ran"
            .to_owned(),
        );
        return notes.looked();
    };
    if routing.execs.is_empty() {
        notes.unaudited(
            "mutant-exec",
            "the recording holds no mutation execution, so no outcome can be held to what ran"
                .to_owned(),
        );
        return notes.looked();
    }
    let mut ran: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for exec in &routing.execs {
        ran.entry(exec.mutant.as_str())
            .or_default()
            .push(exec.outcome.as_str());
    }
    for mutant in recording
        .mutants
        .iter()
        .filter(|mutant| mutant.read_back_from.is_none())
    {
        let mut recorded: Vec<&str> = Vec::new();
        for key in [mutant.display_id.as_str(), mutant.id.as_str()] {
            if let Some(outcomes) = ran.get(key) {
                recorded.extend(outcomes.iter().copied());
            }
        }
        if let Some(why) = contradicted(&mutant.outcome, &recorded) {
            notes.violated(mutant.label(), why);
        } else if mutant.outcome == EQUIVALENT
            && !routing
                .equivalences
                .iter()
                .any(|(display_id, answer)| display_id == &mutant.display_id && answer == IDENTICAL)
        {
            notes.violated(
                mutant.label(),
                "the report says the compiler renders this mutation identically to the code it \
                 mutates, and the recording holds no equivalence answer saying so"
                    .to_owned(),
            );
        }
    }
    notes.looked()
}

/// Why `reported` is not an outcome the executions `recorded` could have come to, if it is not.
fn contradicted(reported: &str, recorded: &[&str]) -> Option<String> {
    let any = |outcome: &str| recorded.contains(&outcome);
    let requires = |outcome: &str| {
        (!any(outcome)).then(|| {
            format!(
                "the report says {reported}, and no recorded execution of it came to \
                 {outcome}: {recorded:?}"
            )
        })
    };
    match reported {
        KILLED | UNCONFIRMED => requires(KILLED),
        WAITED => requires(WAITED),
        STEP_LIMIT_REACHED => requires(STEP_LIMIT_EXEC),
        SURVIVED => (recorded.iter().any(|one| *one != SURVIVED && *one != "not_run")
            || (!recorded.is_empty() && !any(SURVIVED)))
        .then(|| {
            format!(
                "the report says every reaching test ran and none noticed, and the recorded \
                 executions of it came to {recorded:?}; a target whose every test declined \
                 measured nothing and counts neither way, a mutation a proof removed every \
                 execution of has none, and the proofs layer holds that"
            )
        }),
        UNREACHED | REJECTED => (!recorded.is_empty()).then(|| {
            format!(
                "the report says {reported}, which no test ever executes, and the recording holds \
                 executions of it: {recorded:?}"
            )
        }),
        EQUIVALENT | "model-noticed" | "model-proved" => any(KILLED).then(|| {
            format!(
                "the report says {reported}, and a recorded execution of it was killed: a test \
                 told the programs apart"
            )
        }),
        DECLINED => (recorded.is_empty() || recorded.iter().any(|one| *one != "not_run")).then(|| {
            format!(
                "the report says every test that reached it declined to measure, and the recorded \
                 executions of it are not each an execution that measured nothing: {recorded:?}"
            )
        }),
        ERRORED => (!recorded.is_empty()
            && recorded
                .iter()
                .all(|one| *one == SURVIVED || *one == KILLED))
        .then(|| {
            format!(
                "the report says the harness failed, and every recorded execution of it came to \
                 a verdict: {recorded:?}"
            )
        }),
        other => Some(format!(
            "the report says {other}, which is not an outcome this audit knows how to hold to \
             an execution"
        )),
    }
}

fn proofs(
    recording: &Recording<'_>,
    routing: Option<&crate::route::Routing>,
    repairs: &[crate::repair::Repair],
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let Some(routing) = routing else {
        notes.unaudited(
            "route",
            "the run kept no recording of how it routed, so which target each proof removed \
             cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
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
    let licensed = |exec: &&crate::route::Exec| {
        !repairs
            .iter()
            .any(|repair| repair.mutant == exec.mutant && repair.target == exec.target)
    };
    let executed: BTreeSet<String> = routing
        .execs
        .iter()
        .filter(licensed)
        .map(|exec| exec.mutant.clone())
        .collect();
    let ran: Vec<(String, String, String)> = routing
        .execs
        .iter()
        .filter(licensed)
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
        return notes.looked();
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
    notes.looked()
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
    /// What the run concluded, as its recording's `run-end` says; a complete report stores no verdict.
    verdict: Option<String>,
}

impl<'a> Recording<'a> {
    /// The rows every layer reads, each field its schema requires demanded rather than supplied.
    fn of(document: &'a serde_json::Value) -> Result<Self, crate::route::ReadCauseError> {
        use crate::route::required;
        let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
        let targets = rows(document, "targets")
            .iter()
            .map(|row| {
                Ok(TargetRow {
                    id: required(row, "id", text)?,
                    name: required(row, "name", text)?,
                    status: required(row, "status", text)?,
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let mutants = rows(document, "mutants")
            .iter()
            .map(|row| {
                let decision = required(row, "decision", Some)?;
                Ok(MutantRow {
                    id: required(row, "id", text)?,
                    display_id: required(row, "display_id", text)?,
                    catalog_index: row.get("catalog_index").and_then(serde_json::Value::as_u64),
                    outcome: required(decision, "outcome", text)?,
                    acceptance: AcceptanceFact::from_json(row.get("accepted")),
                    killed_by: field(decision, "killed_by"),
                    read_back_from: row
                        .get("reuse")
                        .and_then(|reuse| field(reuse, "source_run_id")),
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let findings = rows(document, "findings")
            .iter()
            .map(|row| {
                Ok(FindingRow {
                    kind: required(row, "kind", text)?,
                    subject: required(row, "subject", text)?,
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let models = rows(document, "models")
            .iter()
            .map(|row| {
                let answer = required(row, "answer", Some)?.clone();
                Ok(ModelRow {
                    mutant: required(row, "mutant", text)?,
                    decision: required(&answer, "decision", text)?,
                    evidence: answer.get("evidence").cloned(),
                    attempt: answer.get("attempt").cloned(),
                    answer,
                    raw: row.clone(),
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        Ok(Self {
            document,
            run_id: required(document, "run_id", text)?,
            contract: required(document, "contract", text)?,
            targets,
            mutants,
            findings,
            shard: document
                .get("scope")
                .and_then(|scope| field(scope, "shard")),
            target: required(required(document, "toolchain", Some)?, "target", text)?,
            models,
            verdict: None,
        })
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
            .filter(|mutant| mutant.outcome == outcome && mutant.read_back_from.is_some())
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

/// A verdict a runner's recording can conclude with, each of which this audit holds to its own rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Concluded {
    /// Every mutation of a full run was answered.
    Assured,
    /// Every mutation of a run over what changed was answered.
    ChangeAssured,
    /// Every mutation of a run over a named scope was answered.
    ScopeAssured,
    /// The code under test broke a contract.
    Defect,
    /// Execution completed and something remains unestablished.
    Insufficient,
    /// One part of a catalog divided between machines, which assures nothing on its own.
    Partial,
    /// The run established nothing.
    Error,
}

impl Concluded {
    /// The name the runner writes.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Assured => "ASSURED",
            Self::ChangeAssured => "CHANGE_ASSURED",
            Self::ScopeAssured => "SCOPE_ASSURED",
            Self::Defect => "DEFECT",
            Self::Insufficient => "INSUFFICIENT",
            Self::Partial => "PARTIAL",
            Self::Error => "ERROR",
        }
    }

    /// The verdict the runner wrote as `name`, or nothing when it names none this audit knows.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|verdict| verdict.name() == name)
    }
}

/// Whether the verdict is one the accounting and the findings support.
fn verdict(recording: &Recording<'_>, audit: &mut Audit) {
    let Some(concluded) = recording.verdict.clone() else {
        Notes::on(audit, Layer::Accounting).unaudited(
            "verdict",
            "the recording does not say what the run concluded, so there is nothing to hold its \
             accounting to"
                .to_owned(),
        );
        return;
    };
    let Some(held) = Concluded::named(&concluded) else {
        Notes::on(audit, Layer::Accounting).violated(
            "verdict",
            format!("the recording concludes {concluded:?}, which is no verdict a runner says"),
        );
        return;
    };
    let shard = recording
        .document
        .get("scope")
        .and_then(|scope| field(scope, "shard"));
    match (held, shard) {
        (Concluded::Assured | Concluded::ChangeAssured | Concluded::ScopeAssured, None) => {
            scope(recording, audit, &concluded);
            answered_throughout(recording, audit, &concluded);
        }
        (Concluded::Assured | Concluded::ChangeAssured | Concluded::ScopeAssured, Some(shard)) => {
            Notes::on(audit, Layer::Accounting).violated(
                "verdict",
                format!(
                    "the recording concludes {concluded} for part {shard} of a divided catalog, \
                     and a part assures nothing on its own"
                ),
            );
        }
        (Concluded::Defect, _) => defect(recording, audit),
        (Concluded::Insufficient, shard) => insufficient(recording, audit, shard.is_some()),
        (Concluded::Partial, Some(_)) => {
            short_of_a_defect(recording, audit, &concluded);
            answered_throughout(recording, audit, &concluded);
        }
        (Concluded::Partial, None) => {
            Notes::on(audit, Layer::Accounting).violated(
                "verdict",
                "the recording concludes PARTIAL and records no shard; PARTIAL is what one part \
                 of a divided catalog concludes"
                    .to_owned(),
            );
        }
        (Concluded::Error, _) => {
            Notes::on(audit, Layer::Accounting).violated(
                "verdict",
                "the recording concludes ERROR beside the report of the same run; a run says \
                 ERROR when it came to no report"
                    .to_owned(),
            );
        }
    }
}

/// An INSUFFICIENT names no defect, and rests on something the run did not establish.
fn insufficient(recording: &Recording<'_>, audit: &mut Audit, part: bool) {
    if short_of_a_defect(recording, audit, Concluded::Insufficient.name()) {
        return;
    }
    let established = recording.findings.is_empty()
        && column(recording.document, "targets", PASSED).is_some_and(|passed| passed > 0)
        && column(recording.document, "mutants", "executed").is_some_and(|executed| executed > 0)
        && recording.mutants.iter().all(answers)
        && !(part && unsettled(recording.document));
    if established {
        Notes::on(audit, Layer::Accounting).violated(
            "verdict",
            "the recording concludes INSUFFICIENT, and it found nothing, observed and asked \
             something, and answered every mutation, so the run established more than it says"
                .to_owned(),
        );
    }
}

/// Whether a part's reach moved or a knob shook, which only the merge settles.
fn unsettled(document: &serde_json::Value) -> bool {
    rows(document, "drift")
        .iter()
        .any(|row| field(row, "state").as_deref() == Some("moved"))
        || rows(document, "knobs").iter().any(|row| {
            matches!(
                row.get("standing")
                    .and_then(|standing| field(standing, "state"))
                    .as_deref(),
                Some("broke" | "moved")
            )
        })
}

/// Whether a mutation row is an answer: decided by a test, a model or the compiler, or accepted as it stands.
fn answers(mutant: &MutantRow) -> bool {
    matches!(
        (mutant.outcome.as_str(), mutant.acceptance),
        (
            REJECTED | KILLED | "model-noticed" | "model-proved" | EQUIVALENT,
            AcceptanceFact::Rejected | AcceptanceFact::Accepted
        ) | (SURVIVED | UNREACHED, AcceptanceFact::Accepted)
    )
}

/// A run that found a defect says DEFECT, whole or in part; whether this one named one.
fn short_of_a_defect(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) -> bool {
    let named: Vec<&str> = recording
        .findings
        .iter()
        .map(|finding| finding.kind.as_str())
        .filter(|kind| DEFECT_KINDS.contains(kind))
        .collect();
    if !named.is_empty() {
        Notes::on(audit, Layer::Accounting).violated(
            "verdict",
            format!(
                "the recording concludes {concluded} and names {}; a run that found a defect \
                 says DEFECT, whole or in part",
                named.join(", ")
            ),
        );
    }
    !named.is_empty()
}

/// What an assurance and a part both claim: nothing was found, something was observed and asked, and every mutation was answered.
fn answered_throughout(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) {
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
                "carries {} findings; it is the claim that nothing was found",
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
        if !answers(mutant) {
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
fn killers(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
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
    notes.looked()
}

/// Whether the mutations nothing noticed and the findings that raise them are the same set.
fn findings(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Findings);
    let mutation_kinds = [
        SURVIVING_MUTANT,
        WAITED_MUTANT,
        STEP_LIMIT_REACHED_MUTANT,
        FAILING_TEST,
        TARGET_MISSING,
        NOT_MEASURED,
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
            (DECLINED, AcceptanceFact::Rejected | AcceptanceFact::Accepted) => Some(NOT_MEASURED),
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
    notes.looked()
}

/// Whether a finding that calls an acceptance unmatched is supported by the complete catalog.
fn acceptances(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
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
    notes.looked()
}

/// Whether every disposition read back from an earlier run names one a reader could go and read, and is one the run's own recording says it read back.
///
/// Re-derived from the runner's route of each: it names the run the report does, nothing of the mutation ran, and a kill's target is one the route still reaches.
/// Whether each target an answer rests on keeps the behaviour key it had, or a carried answer's premises hold, is not in the report, and is left unaudited.
fn reuse(
    recording: &Recording<'_>,
    routing: Option<&crate::route::Routing>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Reuse);
    let mut read_back: Vec<&MutantRow> = Vec::new();
    for mutant in &recording.mutants {
        match mutant.read_back_from.as_deref() {
            Some(run) if run == recording.run_id => notes.violated(
                mutant.label(),
                "the disposition names this run itself as the run it was read back from; a run \
                 cannot have read its own answer back"
                    .to_owned(),
            ),
            Some(_) => read_back.push(mutant),
            None => {}
        }
    }
    if read_back.is_empty() {
        return notes.looked();
    }
    match routing {
        Some(routing) => {
            for mutant in &read_back {
                routed_back(mutant, routing, &mut notes);
            }
        }
        None => notes.unaudited(
            "provenance",
            format!(
                "{} dispositions were read back from an earlier run, and the run kept no \
                 recording of the routes it read them back under",
                read_back.len()
            ),
        ),
    }
    notes.unaudited(
        "provenance",
        format!(
            "{} dispositions were read back from an earlier run; whether each target they rest \
             on keeps the behaviour key it had, or a carried answer's premises hold, is a fact \
             this report does not carry",
            read_back.len()
        ),
    );
    notes.looked()
}

/// Whether the run's own route of `mutant`, a disposition read back, says it read it back from the run the report names, ran none of it, and still reaches the target a kill names.
fn routed_back(mutant: &MutantRow, routing: &crate::route::Routing, notes: &mut Notes<'_>) {
    let Some(route) = routing.route_of(&mutant.id, &mutant.display_id) else {
        notes.violated(
            mutant.label(),
            "the disposition was read back from an earlier run, and the recording holds no \
             route of it to say under what"
                .to_owned(),
        );
        return;
    };
    if route.reused != mutant.read_back_from {
        notes.violated(
            mutant.label(),
            format!(
                "the report says it was read back from {:?}, and the run's own route of it says \
                 {:?}",
                mutant.read_back_from, route.reused
            ),
        );
    }
    if routing
        .execs_for(&mutant.id, &mutant.display_id)
        .next()
        .is_some()
    {
        notes.violated(
            mutant.label(),
            "an answer read back is an execution that did not happen, and the recording holds \
             an execution of it"
                .to_owned(),
        );
    }
    if let Some(killer) = &mutant.killed_by
        && !route.reaching.iter().any(|target| target == killer)
    {
        let word = match route.rule.as_deref() {
            Some("carried") => "filter-differs",
            Some(_) | None => "not-routed",
        };
        notes.violated(
            mutant.label(),
            format!(
                "the kill read back names {killer}, which this run's route no longer reaches \
                 ({word})"
            ),
        );
    }
}

/// The columns of the accounting, against the records they summarise and against the verdict they carry.
fn accounting(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
    target_columns(recording, audit);
    mutant_columns(recording, audit);
    equations(recording, audit);
    verdict(recording, audit);
    Notes::on(audit, Layer::Accounting).looked()
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
