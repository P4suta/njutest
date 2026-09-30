// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed run recorded.
//!
//! [ADR 0004](../../docs/adr/0004-proof-layers-not-budgets.md) ships a proof layer only against a re-implementation that never calls the runner's, so nothing here consults the code that wrote the report: every verdict is re-derived from the recording alone, and wherever the recording does not carry enough to re-derive one, that is said plainly rather than read as agreement.

mod knobs;
pub mod merge;
mod ran;
pub mod sentinel;
pub mod soundness;

use ran::{Executions, Kept, Ran};

use crate::error::Coded as _;
pub use crate::layers::Coverage;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use sha2::Digest as _;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "njutest-assurance-report-v1.json";

/// The record stream a completed run keeps beside its document.
pub const LINES_FILE: &str = "njutest-assurance-report-v1.lines";

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

/// The exit code an audit that found no violation and left something unaudited earns, kept apart so a step that reads only the code cannot take it for an audit that checked everything.
pub const EXIT_UNAUDITED: u8 = 3;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const UNREACHED: &str = "unreached";
const DISCHARGED: &str = "discharged";
const REJECTED: &str = "compile-rejected";
const STEP_LIMIT_REACHED: &str = "step-limit-reached";
const WAITED: &str = "waited";
const UNCONFIRMED: &str = "unconfirmed";
const ERRORED: &str = "errored";
const DECLINED: &str = "declined";
const PASSED: &str = "passed";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNPROVEN_MUTANT: &str = "unproven-mutant";
const UNNOTICED_FAULT: &str = "unnoticed-fault";
const DIMENSION_NOT_MEASURED: &str = "dimension-not-measured";
const CORRUPT_AFTER_CRASH: &str = "corrupt-after-crash";
const BROKEN_UNDER_FAULT: &str = "broken-under-fault";
const NOT_MEASURED_FINDING: &str = "not-measured";
const UNATTRIBUTED_WRITE: &str = "fault-write-unattributed";
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
    /// A field the report's schema requires where it is read is not there, or not the shape it takes.
    #[error("a report with no `{field}` this audit reads, which its schema requires")]
    Absent {
        /// The field.
        field: &'static str,
    },
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
    /// Each kill and wait, held to the control that answered for its test and to what its second run came to.
    Confirmations,
    /// Each mutation's reported outcome, held to the executions of it the recording holds.
    Executions,
    /// What interpreting the suite established, re-derived from what the interpreter said.
    Soundness,
    /// What each row's decision rests on, re-derived from the sealed executions it names or the reasons a lead has none (ADR 0046).
    Evidence,
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
            Self::Confirmations => "confirmations",
            Self::Executions => "executions",
            Self::Soundness => "soundness",
            Self::Evidence => "evidence",
        }
    }

    /// What this layer reads of the executions a row can rest on, which says whether a defect only a sealed execution shows is owed to it.
    #[must_use]
    pub const fn reads(self) -> crate::route::Reads {
        use crate::route::Reads;
        match self {
            Self::Executions | Self::Proofs | Self::Hollow | Self::Reuse => Reads::Both,
            Self::Repair => Reads::Native,
            Self::Accounting
            | Self::Killers
            | Self::Findings
            | Self::Acceptances
            | Self::Wire
            | Self::Model
            | Self::Merge
            | Self::Drift
            | Self::Faults
            | Self::Knobs
            | Self::Crashes
            | Self::Dimensions
            | Self::Concurrency
            | Self::Confirmations
            | Self::Soundness
            | Self::Evidence => Reads::Nothing,
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
    /// What each configured build's engine kept beside its recording of the answers it carried.
    pub beside: &'a [Beside],
    /// The tree the run measured, which every body a carried answer rests on is read again from, or nothing where no `--root` names it.
    pub root: Option<&'a Path>,
}

/// The documents one configured build's engine kept beside its recording of the answers it carried (ADR 0041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beside {
    /// The position of the build's recording among [`Recorded::engines`], whose control records P7 is re-derived from.
    pub engine: usize,
    /// The directory they were read from.
    pub path: String,
    /// `carried-v1.json`: every carried record the build believed, with the plan it was held to.
    pub carried: serde_json::Value,
    /// `skeletons-v1.json`: every item's body digest, sealing and start, and every unit's skeleton.
    pub skeletons: serde_json::Value,
    /// `touched-v1.json`: the guards' record, with the item catalog.
    pub touched: serde_json::Value,
    /// `catalog-v1.json`: every mutation's edit, which each carried record's locus is derived from again.
    pub catalog: serde_json::Value,
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
    let Projected { flat, modeled } = read_report(checkers, path, text)?;
    let mut recording =
        Recording::of(&flat, modeled).map_err(|cause| AuditError::UnreadReport {
            path: path.to_owned(),
            cause,
        })?;
    let runner = checked_runner(recorded.runner, checkers)?;
    recording.verdict = runner.as_ref().and_then(|(_, checked)| concluded(checked));
    let RunnerEvidence {
        routing,
        watched,
        faulted,
        crashed,
        repairs,
        confirmed,
        recorded_executions,
    } = runner_evidence(runner.as_ref())?;
    let engines = engine_evidence(checkers, recorded.engines)?;
    let executions = Executions::read(routing.as_ref(), &engines);
    let rerouted = routing
        .as_ref()
        .zip(repairs.as_deref())
        .map(|(routing, repairs)| Rerouted { routing, repairs });
    let mut audit = Audit {
        run_id: recording.run_id.clone(),
        mutants: recording.mutants.len(),
        targets: recording.targets.len(),
        remarks: Vec::new(),
        coverage: BTreeMap::new(),
    };
    let held = (routing.as_ref(), &executions, engines.as_slice());
    let reusing: Reusing<'_> = (
        routing.as_ref(),
        (recorded.beside, recorded.root),
        &engines,
        &executions,
    );
    for layer in Layer::ALL {
        let Decided(()) = match layer {
            Layer::Accounting => accounting(&recording, &mut audit),
            Layer::Killers => killers(&recording, &mut audit),
            Layer::Findings => findings(&recording, &mut audit),
            Layer::Acceptances => acceptances(&recording, &mut audit),
            Layer::Reuse => reuse(&recording, reusing, &mut audit),
            Layer::Proofs => proofs(&recording, rerouted, &executions, &mut audit),
            Layer::Executions => executions_held(&recording, held, &mut audit),
            Layer::Hollow => hollow(&recording, &executions, &mut audit),
            Layer::Wire => wire(&recording, watched.as_ref(), &mut audit),
            Layer::Model => models(&recording, run, &mut audit),
            Layer::Merge => Notes::on(&mut audit, Layer::Merge)
                .absent("this report is one run's, and a merge is audited against its shards"),
            Layer::Drift => drift(&recording, (&engines, rerouted), &mut audit),
            Layer::Repair => repaired(&recording, (&engines, rerouted), &executions, &mut audit),
            Layer::Faults => faults(&recording, faulted.as_ref(), &mut audit),
            Layer::Dimensions => dimensions(&recording, run, &mut audit),
            Layer::Crashes => crashes(&recording, crashed.as_ref(), &mut audit),
            Layer::Knobs => knobs::audited(&recording, &engines, &mut audit),
            Layer::Concurrency => concurrency(&recording, &engines, &mut audit),
            Layer::Confirmations => confirmations(&recording, confirmed.as_ref(), &mut audit),
            Layer::Soundness => soundness::audited(
                &recording,
                recorded_executions.as_deref(),
                recorded.outputs,
                &mut audit,
            ),
            Layer::Evidence => evidence(&recording, &mut audit),
        };
    }
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

/// The runner's recording, where the run kept one, read and checked against the runner's schema.
fn checked_runner<'a>(
    runner: Option<(&'a str, &'a str)>,
    checkers: &crate::schemas::Checkers,
) -> Result<Option<(&'a str, crate::route::Checked<crate::schemas::RunnerLines>)>, AuditError> {
    runner
        .map(|(recording_path, text)| {
            crate::route::Checked::read(text, checkers)
                .map(|checked| (recording_path, checked))
                .map_err(|source| AuditError::MalformedRecording {
                    path: recording_path.to_owned(),
                    source,
                })
        })
        .transpose()
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
                sealed: sealed_runs(checked.events()),
                controls: match crate::route::read(&checked) {
                    Ok(routing) => Some(routing.controls),
                    Err(_unreadable) => None,
                },
            })
        })
        .collect()
}

/// Every sealed execution `events` hold, as the mutant it ran and the execution, in the order they hold them, or nothing where one of them lacks a field.
fn sealed_runs(events: &[serde_json::Value]) -> Option<Vec<(String, SealedRun)>> {
    events
        .iter()
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("sealed-exec")
        })
        .map(|event| {
            let sealed = event.get("sealed")?;
            Some((
                field(sealed, "mutant")?,
                SealedRun {
                    target: field(sealed, "target")?,
                    test: field(sealed, "test")?,
                    came_to: field(sealed, "came_to")?,
                },
            ))
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
        repairs: read_runner(runner, crate::repair::read)?,
        confirmed: read_runner(runner, crate::confirm::read)?,
        recorded_executions: runner.map(|(_, checked)| executions_of(checked)),
    })
}

/// What the runner's recording says, read once.
struct RunnerEvidence {
    routing: Option<crate::route::Routing>,
    watched: Option<crate::wire::Watched>,
    faulted: Option<crate::faults::Faulted>,
    crashed: Option<crate::crashes::Crashed>,
    repairs: Option<Vec<crate::repair::Repair>>,
    confirmed: Option<crate::confirm::Confirmations>,
    recorded_executions: Option<Vec<serde_json::Value>>,
}

/// How the runner's recording routed, and every disposition it ran again against a target whose reach moved, which that one recording says together.
#[derive(Debug, Clone, Copy)]
struct Rerouted<'a> {
    routing: &'a crate::route::Routing,
    repairs: &'a [crate::repair::Repair],
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
) -> Result<Projected, AuditError> {
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

/// The flat view of the one part of one configured build that every layer re-decides, and what the report says of model checking.
struct Projected {
    /// A complete report's one build measured whole, or a shard's one build with the shard it is written into its scope.
    flat: serde_json::Value,
    /// What the report says of model checking, whose records the flat view holds as `models`.
    modeled: Modeled,
}

/// What a report says of model checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modeled {
    /// A shard's report, which carries no model completion.
    Shard,
    /// The run was not required to model any mutant.
    NotRequired,
    /// The run modeled the mutants of its batch.
    Verified,
}

/// The array field `key` of `value`, which the report's schema requires where this reads it.
///
/// # Errors
/// [`UnprojectableError::Absent`] where it is not there or is not an array.
fn required_list<'a>(
    value: &'a serde_json::Value,
    key: &'static str,
) -> Result<&'a [serde_json::Value], UnprojectableError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .ok_or(UnprojectableError::Absent { field: key })
}

/// What `report` says of model checking, with the records of its batch where it modeled one; a shard's report carries no completion.
///
/// # Errors
/// [`UnprojectableError::Absent`] for a complete report whose completion, its kind, or its batch is not there.
fn modeled(
    report: &serde_json::Value,
    shard: bool,
) -> Result<(Modeled, Vec<serde_json::Value>), UnprojectableError> {
    if shard {
        return Ok((Modeled::Shard, Vec::new()));
    }
    let completion = report
        .get("model_completion")
        .ok_or(UnprojectableError::Absent {
            field: "model_completion",
        })?;
    match completion.get("kind").and_then(serde_json::Value::as_str) {
        Some("not-required") => Ok((Modeled::NotRequired, Vec::new())),
        Some("verified") => {
            let batch = completion
                .get("batch")
                .ok_or(UnprojectableError::Absent { field: "batch" })?;
            Ok((Modeled::Verified, required_list(batch, "records")?.to_vec()))
        }
        _ => Err(UnprojectableError::Absent { field: "kind" }),
    }
}

/// The one part of the one configured build `report` holds: a shard's own source, or a complete report's one part.
///
/// # Errors
/// [`UnprojectableError::Builds`] or [`UnprojectableError::Parts`] for another number than one, and [`UnprojectableError::Absent`] for a list or a source that is not there.
fn the_part(
    report: &serde_json::Value,
    shard: bool,
) -> Result<serde_json::Value, UnprojectableError> {
    let builds = required_list(report, "builds")?;
    let [build] = builds else {
        return Err(UnprojectableError::Builds {
            count: builds.len(),
        });
    };
    if shard {
        return build
            .get("source")
            .cloned()
            .ok_or(UnprojectableError::Absent { field: "source" });
    }
    let parts = required_list(build, "parts")?;
    let [part] = parts else {
        return Err(UnprojectableError::Parts { count: parts.len() });
    };
    Ok(part.clone())
}

/// The flat view of the one part of one configured build that every layer re-decides, with what the report says of model checking.
fn projected(document: &serde_json::Value) -> Result<Projected, UnprojectableError> {
    let Some(kind) = document.get("document_type") else {
        return Err(UnprojectableError::Flat);
    };
    let shard = kind.as_str() == Some("shard");
    let mut report = document
        .get("report")
        .cloned()
        .ok_or(UnprojectableError::Absent { field: "report" })?;
    let part = the_part(&report, shard)?;
    let (modeled, models) = modeled(&report, shard)?;
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
        "provenance",
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
        "repaired",
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
    let findings: Vec<serde_json::Value> = required_list(&report, "global_findings")?
        .iter()
        .chain(required_list(part, "findings")?)
        .cloned()
        .collect();
    flat.insert("findings".to_owned(), serde_json::Value::Array(findings));
    flat.insert("models".to_owned(), serde_json::Value::Array(models));
    Ok(Projected {
        flat: serde_json::Value::Object(flat),
        modeled,
    })
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
    let rows = recording.part.concurrency;
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
    /// Every sealed execution, as the mutant it ran and the execution, in the order recorded; nothing where one of them cannot be read.
    sealed: Option<Vec<(String, SealedRun)>>,
    /// Every sealed control, in the order recorded; nothing where one of them cannot be read.
    controls: Option<Vec<crate::route::SealedControl>>,
}

/// Each target and state the report's drift records name; a record that names no target or no state is not the shape a run writes it in, which is said.
fn recorded_drift(recording: &Recording<'_>, notes: &mut Notes<'_>) -> Vec<(String, String)> {
    let mut recorded = Vec::new();
    for row in recording.part.drift {
        let (Some(target), Some(state)) = (field(row, "target"), field(row, "state")) else {
            notes.violated(
                "drift",
                "a drift record of the report names no target or no state, which is not the \
                 shape a run writes it in"
                    .to_owned(),
            );
            continue;
        };
        recorded.push((target, state));
    }
    recorded
}

fn drift(
    recording: &Recording<'_>,
    (engines, rerouted): (&[Engine], Option<Rerouted<'_>>),
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Drift);
    let recorded = recorded_drift(recording, &mut notes);
    let touched = match engines {
        [] => {
            notes.unaudited(
                "drift",
                "the run kept no engine recording, so which targets moved between their \
                 baseline and a control cannot be re-derived"
                    .to_owned(),
            );
            return notes.looked();
        }
        [one] => &one.touched,
        several => {
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
    let resting = resting(recording, &derived, rerouted);
    held_to_findings(recording, &resting, &mut notes);
    held_to_limitation(recording, &derived, &mut notes);
    held_to_repairs(recording, &resting, &mut notes);
    if let Some(rerouted) = rerouted {
        held_to_counts(recording, &derived, rerouted, &mut notes);
    }
    notes.looked()
}

/// The counts the `unstable-baseline` finding and the `reach-moved` limitation give each moved target, re-derived from the rows, their routes and the repairs: how many survivors and how many unreached claims still rest on it, and how many dispositions resting on it a repair replaced (ADR 0036 decision 3).
fn held_to_counts(
    recording: &Recording<'_>,
    derived: &BTreeMap<String, crate::drift::Standing>,
    Rerouted { routing, repairs }: Rerouted<'_>,
    notes: &mut Notes<'_>,
) {
    let moved = derived
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| target);
    for target in moved {
        let still = |outcome: &str| {
            recording
                .mutants
                .iter()
                .filter(|row| row.outcome == outcome)
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
                            && repair.settles()
                    })
                })
                .count()
        };
        let counted = format!(
            ": {} a proof removed its run of, and {} no test reached, rest on it",
            mutations(still(SURVIVED)),
            mutations(still(UNREACHED))
        );
        for detail in recording
            .part
            .findings
            .iter()
            .filter(|row| {
                field(row, "kind").as_deref() == Some(UNSTABLE_BASELINE)
                    && field(row, "subject").as_deref() == Some(target.as_str())
            })
            .filter_map(|row| field(row, "detail"))
            .filter(|detail| !detail.contains(&counted))
        {
            notes.violated(
                target,
                format!(
                    "the {UNSTABLE_BASELINE} finding about {target} does not say what the rows \
                     and repairs leave resting on it, which is `{}`: {detail}",
                    counted.trim_start_matches(": ")
                ),
            );
        }
        held_to_replaced(recording, (target, repairs), notes);
    }
}

/// The `reach-moved` limitation about `target`, held to how many dispositions resting on it a repair replaced.
fn held_to_replaced(
    recording: &Recording<'_>,
    (target, repairs): (&str, &[crate::repair::Repair]),
    notes: &mut Notes<'_>,
) {
    let again = crate::repair::replaced_against(repairs, target);
    let said = format!(
        "; {again} {} that rested on its baseline",
        if again == 1 {
            "disposition"
        } else {
            "dispositions"
        }
    );
    for detail in recording
        .part
        .limitations
        .iter()
        .filter(|row| field(row, "name").as_deref() == Some(REACH_MOVED))
        .filter_map(|row| field(row, "detail"))
        .filter(|detail| detail.ends_with(&format!("({target})")))
        .filter(|detail| !detail.contains(&said))
    {
        notes.violated(
            target,
            format!(
                "the {REACH_MOVED} limitation about {target} does not count the {again} \
                 disposition(s) the repairs replaced there: {detail}"
            ),
        );
    }
}

/// How many mutations, in the words a finding counts them in.
fn mutations(count: usize) -> String {
    if count == 1 {
        "1 mutation".to_owned()
    } else {
        format!("{count} mutations")
    }
}

/// How many dispositions still rest on each moved target: a survivor or an unreached claim whose route did not put the target to it, and which no repair against it that reached the site decided again; every one where the run kept no routing to tell.
fn resting(
    recording: &Recording<'_>,
    derived: &BTreeMap<String, crate::drift::Standing>,
    rerouted: Option<Rerouted<'_>>,
) -> BTreeMap<String, Option<usize>> {
    derived
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| {
            let count = rerouted.map(|Rerouted { routing, repairs }| {
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
                                && repair.settles()
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
    let stated = limitation_targets(recording, REACH_MOVED, notes);
    for (target, count) in resting {
        let named = stated
            .iter()
            .any(|targets| targets.iter().any(|one| one == target));
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
    (engines, rerouted): (&[Engine], Option<Rerouted<'_>>),
    executions: &Executions<'_>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Repair);
    let Some(Rerouted { routing, repairs }) = rerouted else {
        let moved = recording
            .part
            .drift
            .iter()
            .filter(|row| field(row, "state").as_deref() == Some("moved"))
            .count();
        if moved == 0 {
            return notes.absent("the report records no target whose reach moved");
        }
        notes.unaudited(
            "repair",
            format!(
                "the report records {moved} moved target(s) and the run kept no recording, so \
                 which dispositions it ran again against them cannot be re-derived"
            ),
        );
        return notes.looked();
    };
    let [
        Engine {
            touched, sealed, ..
        },
    ] = engines
    else {
        if repairs.is_empty() {
            return notes
                .absent("the run ran no disposition again against a target whose reach moved");
        }
        notes.unaudited(
            "repair",
            format!(
                "the report rests on {} repair(s), and the recording holds not exactly one engine \
                 recording to re-derive them from",
                repairs.len()
            ),
        );
        return notes.looked();
    };
    let moved = crate::drift::standings(touched);
    counted(recording, repairs, &moved, &mut notes);
    let owed = owed(recording, (repairs, routing), &moved, &mut notes);
    if repairs.is_empty() && owed == 0 {
        return notes.absent("the run ran no disposition again against a target whose reach moved");
    }
    let paired = paired(recording, repairs, (executions, touched), &mut notes);
    for repair in repairs {
        match &repair.by {
            crate::repair::By::Native => {
                one_repair(recording, (repair, &moved, routing), &paired, &mut notes);
            }
            crate::repair::By::Sealed(put) => one_sealed(
                recording,
                (repair, put, &moved, routing),
                sealed.as_deref(),
                &mut notes,
            ),
        }
    }
    for row in &recording.mutants {
        let of_it: Vec<&crate::repair::Repair> = repairs
            .iter()
            .filter(|one| one.mutant == row.display_id)
            .collect();
        if let Some(joined) = crate::repair::joined(&of_it)
            && row.outcome.replace('_', "-") != joined
        {
            notes.violated(
                &row.display_id,
                format!(
                    "the runs of {} again against the moved targets it rested on come to {joined} \
                     together, and the report says {}",
                    row.display_id, row.outcome
                ),
            );
        }
    }
    notes.looked()
}

/// Whether the part's `repaired` records say, of each target the touch records show moving and of nothing else, how many dispositions the repair records replaced there: those whose run reached the site or came to something other than what the disposition was (ADR 0036 decision 4).
fn counted(
    recording: &Recording<'_>,
    repairs: &[crate::repair::Repair],
    moved: &BTreeMap<String, crate::drift::Standing>,
    notes: &mut Notes<'_>,
) {
    let mut said: Vec<(String, u64)> = Vec::new();
    for row in recording.part.repaired {
        let (Some(target), Some(again)) = (
            field(row, "target"),
            row.get("again").and_then(serde_json::Value::as_u64),
        ) else {
            notes.violated(
                "repaired",
                "a repair count of the report names no target or no count, which is not the \
                 shape a run writes it in"
                    .to_owned(),
            );
            continue;
        };
        said.push((target, again));
    }
    let owed = moved
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| target);
    for target in owed {
        let replaced = crate::repair::replaced_against(repairs, target);
        let stated: Vec<u64> = said
            .iter()
            .filter(|(named, _)| named == target)
            .map(|(_, again)| *again)
            .collect();
        match stated.as_slice() {
            [again] if u64::try_from(replaced) == Ok(*again) => {}
            [again] => notes.violated(
                target,
                format!(
                    "the report counts {again} disposition(s) run again against {target}, and \
                     its repair records replaced {replaced}"
                ),
            ),
            others => notes.violated(
                target,
                format!(
                    "the reach of {target} moved and the report holds {} repair count(s) about it \
                     where it owes exactly one",
                    others.len()
                ),
            ),
        }
    }
    for (target, again) in &said {
        if moved.get(target) != Some(&crate::drift::Standing::Moved) {
            notes.violated(
                target,
                format!(
                    "the report counts {again} disposition(s) run again against {target}, whose \
                     reach the touch records do not show moving"
                ),
            );
        }
    }
}

/// How many dispositions rested on a moved target, each of which the repair owes a run against that target, holding every such pair to a repair that names it and every repair to the order the repair takes: mutation by mutation in catalog order, its sealed puts before its native runs, and moved target by moved target in name order (ADR 0036 decision 1).
///
/// A disposition rests on a target when its route did not put the target to it and it was `survived` or `unreached` before any repair.
/// A sealed verdict is owed a sealed put against every such target, and a lead a native run against each, whatever a run against another made of it, until one against a target earlier in name order kills it; a sealed verdict a put left a lead is owed those native runs where what the native run judged of it is `survived` or `unreached`.
fn owed(
    recording: &Recording<'_>,
    (repairs, routing): (&[crate::repair::Repair], &crate::route::Routing),
    moved: &BTreeMap<String, crate::drift::Standing>,
    notes: &mut Notes<'_>,
) -> usize {
    let mut owed: Vec<(&str, &str)> = Vec::new();
    let targets = moved
        .iter()
        .filter(|(_, standing)| **standing == crate::drift::Standing::Moved)
        .map(|(target, _)| target);
    for target in targets {
        for row in &recording.mutants {
            let put = routing.routes.iter().any(|route| {
                route.names(&row.id, &row.display_id)
                    && route.reaching.iter().any(|one| one == target)
            });
            let of_it = || repairs.iter().filter(|one| one.mutant == row.display_id);
            let then = match of_it().next() {
                Some(first) => first.was.as_str(),
                None => row.outcome.as_str(),
            };
            if put || !matches!(then, SURVIVED | UNREACHED) {
                continue;
            }
            let judged = of_it()
                .find(|one| {
                    matches!(
                        one.by,
                        crate::repair::By::Sealed(crate::repair::Put::Unproven(_))
                    )
                })
                .map(|one| one.now.as_str());
            let (by_seal, natively) = match (&row.rests, judged) {
                (Rests::Sealed(_), _) => (true, false),
                (Rests::Unproven(_), Some(judged)) => {
                    (true, matches!(judged, SURVIVED | UNREACHED))
                }
                (Rests::Unproven(_), None) => (false, true),
                (Rests::Nothing, _) => (false, false),
            };
            let killed_before =
                of_it().any(|one| !one.sealed() && one.target < *target && one.now == KILLED);
            let debts = [
                (by_seal, true, "no sealed put ran it again there"),
                (
                    natively && !killed_before,
                    false,
                    "no repair ran it again there",
                ),
            ];
            for (owing, sealed_put, missing) in debts {
                if !owing {
                    continue;
                }
                owed.push((target.as_str(), row.display_id.as_str()));
                if !of_it().any(|one| one.target == *target && one.sealed() == sealed_put) {
                    notes.violated(
                        &row.display_id,
                        format!(
                            "it was {then} on the word of the baseline of {target}, whose reach \
                             moved, and {missing}"
                        ),
                    );
                }
            }
        }
    }
    in_order(recording, repairs, notes);
    owed.len()
}

/// Whether `repairs` ran in the order the repair takes: mutation by mutation in catalog order, its sealed puts before its native runs, and moved target by moved target in name order.
fn in_order(recording: &Recording<'_>, repairs: &[crate::repair::Repair], notes: &mut Notes<'_>) {
    let place = |repair: &crate::repair::Repair| {
        (
            recording
                .mutants
                .iter()
                .find(|row| row.display_id == repair.mutant)
                .and_then(|row| row.catalog_index),
            !repair.sealed(),
            repair.target.clone(),
        )
    };
    for (earlier, later) in repairs.iter().zip(repairs.iter().skip(1)) {
        if place(earlier) > place(later) {
            notes.violated(
                &later.mutant,
                format!(
                    "it was run again against {} after {} was against {}, out of the order the \
                     repair takes: mutation by mutation in catalog order, its sealed puts before \
                     its native runs, and moved target by moved target in name order",
                    later.target, earlier.mutant, earlier.target
                ),
            );
        }
    }
}

/// Each repair's last execution against its moved target, paired with the last engine repair touch record naming that mutation and target, which is the quiet re-measurement's where a wait had one; a repair touch no repair names is a violation.
fn paired<'a>(
    recording: &Recording<'_>,
    repairs: &[crate::repair::Repair],
    (executions, touched): (&Executions<'a>, &'a crate::drift::Touched),
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
        let claimed = repairs
            .iter()
            .filter(|repair| !repair.sealed())
            .any(|repair| {
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
    let native: Vec<&crate::route::Exec> = executions
        .each()
        .filter_map(|(_, ran)| run_again_natively(ran))
        .collect();
    let mut pairs = Vec::new();
    for repair in repairs.iter().filter(|repair| !repair.sealed()) {
        let id = full(&repair.mutant);
        let Some(exec) = native.iter().rev().copied().find(|exec| {
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
        pairs.push((exec, naming.last().copied()));
    }
    pairs
}

/// The execution `ran` is where a repair could be it: a repair runs with sealing off (ADR 0036), so a sealed execution is the mutation phase's and never a repair's.
const fn run_again_natively(ran: Ran<'_>) -> Option<&crate::route::Exec> {
    match ran {
        Ran::Native(exec) => Some(exec),
        Ran::Sealed(_) => None,
    }
}

/// Whether a repair's disposition rested on its target: the target moved, the route did not put it, and what it was is what its route made it before any repair, which every run of it again starts from.
fn rested(
    repair: &crate::repair::Repair,
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
    let (expected_was, by) = if route
        .is_some_and(|route| route.reaching.is_empty() && route.granularity != DISCHARGED)
    {
        (
            UNREACHED,
            "its route reaches no target and a proof removed none, which makes it",
        )
    } else {
        (SURVIVED, "its route makes it")
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

/// One sealed put of a verdict again against a moved target, held to what its target and route decide, to the verdict its own executions establish, to the sealed executions the engine recorded of the mutation, and to the row it left (ADR 0036 decision 1).
fn one_sealed(
    recording: &Recording<'_>,
    (repair, put, moved, routing): (
        &crate::repair::Repair,
        &crate::repair::Put,
        &BTreeMap<String, crate::drift::Standing>,
        &crate::route::Routing,
    ),
    recorded: Option<&[(String, SealedRun)]>,
    notes: &mut Notes<'_>,
) {
    let subject = &repair.mutant;
    rested(repair, (moved, routing), notes);
    let row = recording
        .mutants
        .iter()
        .find(|row| row.display_id == *subject);
    match put {
        crate::repair::Put::Established(runs) => {
            reestablished((repair, runs), row, recorded, notes);
        }
        crate::repair::Put::Unproven(reasons) => {
            if let Some(why) = unreasoned(reasons) {
                notes.violated(subject, format!("its sealed put again: {why}"));
            }
            if repair.reached != "not-reached" {
                notes.violated(
                    subject,
                    format!(
                        "its sealed put again established nothing, and the repair says it {} the \
                         site",
                        repair.reached
                    ),
                );
            }
            if row.is_some_and(|row| !matches!(row.rests, Rests::Unproven(_))) {
                notes.violated(
                    subject,
                    "its sealed put again established nothing, so it is a lead, and the report \
                     rests it on sealed executions"
                        .to_owned(),
                );
            }
        }
    }
}

/// A sealed put again that re-established a verdict on `runs`, held to the verdict they establish, to whether one of them is of its target, to the sealed executions the engine `recorded` of the mutation, and to the evidence of its `row`.
fn reestablished(
    (repair, runs): (&crate::repair::Repair, &[(String, String, String)]),
    row: Option<&MutantRow>,
    recorded: Option<&[(String, SealedRun)]>,
    notes: &mut Notes<'_>,
) {
    let subject = &repair.mutant;
    let runs: Vec<SealedRun> = runs
        .iter()
        .map(|(target, test, came_to)| SealedRun {
            target: target.clone(),
            test: test.clone(),
            came_to: came_to.clone(),
        })
        .collect();
    let killed_by = runs
        .iter()
        .find(|run| DETECTIONS.contains(&run.came_to.as_str()))
        .map(|run| run.target.as_str());
    if let Some(why) = sealed_against(&runs, &repair.now, killed_by) {
        notes.violated(subject, format!("its sealed put again: {why}"));
    }
    let reached = if runs.iter().any(|run| run.target == repair.target) {
        "reached"
    } else {
        "not-reached"
    };
    if repair.reached != reached {
        notes.violated(
            subject,
            format!(
                "the repair says its sealed put {} the site against {}, and its executions say \
                 {reached}",
                repair.reached, repair.target
            ),
        );
    }
    if let (Some(recorded), Some(row)) = (recorded, row) {
        let engine: Vec<&SealedRun> = recorded
            .iter()
            .filter(|(mutant, _)| *mutant == row.display_id || *mutant == row.id)
            .map(|(_, run)| run)
            .collect();
        if let Some(why) = unrecorded_sealed(&runs, engine) {
            notes.violated(
                subject,
                format!(
                    "its sealed put again is not the executions its verdict was established by: \
                     {why}"
                ),
            );
        }
    }
    if row.is_some_and(|row| row.rests != Rests::Sealed(runs.clone())) {
        notes.violated(
            subject,
            "its sealed put again re-established a verdict, and the report does not rest on the \
             executions that put came to"
                .to_owned(),
        );
    }
}

/// One repair held to what its target, route, last execution and touch record decide.
fn one_repair(
    recording: &Recording<'_>,
    (repair, moved, routing): (
        &crate::repair::Repair,
        &BTreeMap<String, crate::drift::Standing>,
        &crate::route::Routing,
    ),
    paired: &[(&crate::route::Exec, Option<&crate::drift::Touch>)],
    notes: &mut Notes<'_>,
) {
    let subject = &repair.mutant;
    rested(repair, (moved, routing), notes);
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
    let stated = limitation_targets(recording, DRIFT_NOT_MEASURED, notes);
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
        (_, [targets]) => {
            for target in &owed {
                if !targets.iter().any(|one| one == target) {
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

/// The targets a limitation's detail names in its closing list, which is how a report names the targets a limitation is about, or nothing where the detail closes on no list.
fn named(detail: &str) -> Option<Vec<&str>> {
    let (_, list) = detail.rsplit_once(" (")?;
    Some(list.strip_suffix(')')?.split(", ").collect())
}

/// The targets each limitation named `name` names, one list for each; a limitation whose detail says nothing or closes on no list names nothing a reader can hold it to, which is said.
fn limitation_targets(
    recording: &Recording<'_>,
    name: &str,
    notes: &mut Notes<'_>,
) -> Vec<Vec<String>> {
    let mut stated = Vec::new();
    for row in recording
        .part
        .limitations
        .iter()
        .filter(|row| field(row, "name").as_deref() == Some(name))
    {
        let detail = field(row, "detail");
        let Some(targets) = detail.as_deref().and_then(named) else {
            notes.violated(
                name,
                format!(
                    "a {name} limitation names no target in its closing list, which is not the \
                     shape a run writes it in"
                ),
            );
            continue;
        };
        stated.push(targets.into_iter().map(str::to_owned).collect());
    }
    stated
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
    edit: RowEdit,
    outcome: String,
    acceptance: AcceptanceFact,
    killed_by: Option<String>,
    read_back_from: Option<String>,
    rests: Rests,
}

/// Where a row says its mutation's edit is, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RowEdit {
    path: String,
    line: u64,
    column: u64,
    original: String,
    replacement: String,
}

impl RowEdit {
    /// The edit `row` says, from its required columns.
    fn read(row: &serde_json::Value) -> Option<Self> {
        let position = row.get("position")?;
        Some(Self {
            path: row.get("path")?.as_str()?.to_owned(),
            line: position.get("line")?.as_u64()?,
            column: position.get("column")?.as_u64()?,
            original: row.get("original")?.as_str()?.to_owned(),
            replacement: row.get("replacement")?.as_str()?.to_owned(),
        })
    }
}

/// One sealed execution, as a report's evidence names it and an engine recording holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SealedRun {
    /// The target whose module ran.
    target: String,
    /// The test it ran.
    test: String,
    /// What it came to.
    came_to: String,
}

/// What a row says its decision rests on, as its `evidence` says it (ADR 0046).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rests {
    /// No execution: the compiler refused the mutation.
    Nothing,
    /// The sealed executions it names, in the order they ran.
    Sealed(Vec<SealedRun>),
    /// No sealed execution established anything, for every reason it names.
    Unproven(Vec<String>),
}

impl Rests {
    /// What `evidence` says, where it is in a shape a run writes it in.
    fn read(evidence: &serde_json::Value) -> Option<Self> {
        if evidence.is_null() {
            return Some(Self::Nothing);
        }
        match evidence.get("kind")?.as_str()? {
            "sealed" => evidence
                .get("executions")?
                .as_array()?
                .iter()
                .map(|execution| {
                    Some(SealedRun {
                        target: field(execution, "target")?,
                        test: field(execution, "test")?,
                        came_to: field(execution, "came_to")?,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .map(Self::Sealed),
            "unproven" => evidence
                .get("reasons")?
                .as_array()?
                .iter()
                .map(|reason| reason.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .map(Self::Unproven),
            _ => None,
        }
    }
}

/// The outcomes only a sealed execution establishes, which a native run's say-so leaves a lead.
const SEALED_OUTCOMES: [&str; 4] = [KILLED, SURVIVED, UNREACHED, EQUIVALENT];

/// What a sealed execution comes to where it detected the mutation.
pub const DETECTIONS: [&str; 6] = [
    "panicked",
    "failed",
    "trapped",
    "fuel-exceeded",
    "memory-exceeded",
    "declined",
];

/// What a sealed execution comes to where it established nothing, which no execution beside it can make a verdict of but a detection.
pub const DOUBTS: [&str; 5] = [
    "exited-early",
    "stack-overflow",
    "refused",
    "unaccounted",
    "unmatched",
];

/// What a sealed execution comes to where the test declined to measure in the words its control declined in, which measures nothing and is set aside (ADR 0043).
pub const SET_ASIDE: &str = "set-aside";

/// Every reason a sealed run gives for establishing no verdict.
pub const REASONS: [&str; 11] = [
    "not-sealed",
    "native",
    "guard-absent",
    "test-absent",
    "reach-differs",
    "exited-early",
    "stack-overflow",
    "refused",
    "unaccounted",
    "declined",
    "unmatched",
];

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

    /// Whether what the row says is a lead: an outcome only a sealed execution establishes, which none did.
    fn lead(&self) -> bool {
        matches!(self.rests, Rests::Unproven(_)) && SEALED_OUTCOMES.contains(&self.outcome.as_str())
    }

    /// Whether the row is a verdict sealed executions established, which owes no native record of itself.
    fn sealed(&self) -> bool {
        matches!(self.rests, Rests::Sealed(_)) && SEALED_OUTCOMES.contains(&self.outcome.as_str())
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
    match (recording.modeled, recording.contract == "verified-v1") {
        (Modeled::Verified, false) => notes.violated(
            "models",
            format!(
                "contract {:?} cannot carry a verified-v1 model batch",
                recording.contract
            ),
        ),
        (Modeled::NotRequired, true) => notes.violated(
            "models",
            "the contract is verified-v1, and the report says no mutant was required to be \
             modeled"
                .to_owned(),
        ),
        (Modeled::Verified, true) | (Modeled::NotRequired, false) | (Modeled::Shard, _) => {}
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
/// The targets the sealed executions say noticed nothing, held to the findings that name them.
///
/// Re-derived from the sealed executions the engine recorded, as the runner decides it (ADR 0046): of a row resting on sealed executions, a target answered where its executions detected the mutation or passed, and noticed it where one detected it.
/// A target whose executions established nothing, or were set aside for declining as their controls did, gave no answer to hold against it, and a native execution is a lead, which answers nothing.
///
/// A part of a catalog is not held to this at all.
/// Whether a target notices anything is a statement about the whole catalog, and a part has seen a slice: a target silent in this part may have noticed something in another,
/// and demanding a finding here would demand one the whole would contradict.
fn hollow(recording: &Recording<'_>, executions: &Executions<'_>, audit: &mut Audit) -> Decided {
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
    match executions.sealed {
        Kept::Held => {}
        Kept::Unrecorded => {
            notes.unaudited(
                "sealed-exec",
                "the run kept no engine recording, so which targets sealed executions put to a \
                 mutation and which noticed none cannot be re-derived"
                    .to_owned(),
            );
            return notes.looked();
        }
        Kept::Unreadable => {
            notes.unaudited(
                "sealed-exec",
                "an engine recording holds a sealed execution this audit cannot read, so which \
                 targets noticed nothing cannot be re-derived"
                    .to_owned(),
            );
            return notes.looked();
        }
    }
    let asked = match answering(recording, executions) {
        Ok(asked) => asked,
        Err(Uncounted::Overflow { target }) => {
            notes.violated(
                target,
                "the execution count exceeds the report wire's u64 range".to_owned(),
            );
            return notes.looked();
        }
        Err(Uncounted::Unplaced { target, came_to }) => {
            notes.violated(
                target,
                format!(
                    "a sealed execution of {target} came to {came_to:?}, which is no ending a \
                     sealed execution comes to"
                ),
            );
            return notes.looked();
        }
    };
    hollow_held_to_findings(&asked, &named_hollow_targets(recording), &mut notes);
    notes.looked()
}

/// Holds the hollow-target findings `named` to the targets `asked` says answered about a mutation and noticed none.
fn hollow_held_to_findings(
    asked: &BTreeMap<&str, (u64, bool)>,
    named: &BTreeSet<&str>,
    notes: &mut Notes<'_>,
) {
    let owed: BTreeSet<&str> = asked
        .iter()
        .filter(|(_, (count, noticed))| *count > 0 && !*noticed)
        .map(|(target, _)| *target)
        .collect();
    for target in owed.difference(named) {
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

/// Why the answers of the sealed executions could not be counted, which the layer states as a violation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Uncounted<'a> {
    /// A target answered about more mutations than the report wire counts.
    Overflow { target: &'a str },
    /// A sealed execution came to an ending this audit does not know.
    Unplaced { target: &'a str, came_to: &'a str },
}

/// What one target's sealed executions of one mutation said, weakest first: set aside, passed, established nothing, detected it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum SealedAnswer {
    /// Its test declined as its control did, which measures nothing.
    SetAside,
    /// Its test passed with the mutation in place.
    Passed,
    /// Its execution established nothing.
    Doubted,
    /// Its execution detected the mutation.
    Detected,
}

impl SealedAnswer {
    /// What an execution that came to `came_to` said, where it is an ending a sealed execution comes to.
    fn of(came_to: &str) -> Option<Self> {
        if DETECTIONS.contains(&came_to) {
            Some(Self::Detected)
        } else if DOUBTS.contains(&came_to) {
            Some(Self::Doubted)
        } else if came_to == PASSED {
            Some(Self::Passed)
        } else if came_to == SET_ASIDE {
            Some(Self::SetAside)
        } else {
            None
        }
    }
}

/// How many mutations each target answered about and whether it noticed one, over every row resting on sealed executions, each target's answer to a row being the strongest thing its executions of it said.
fn answering<'a>(
    recording: &Recording<'_>,
    executions: &Executions<'a>,
) -> Result<BTreeMap<&'a str, (u64, bool)>, Uncounted<'a>> {
    let mut asked: BTreeMap<&str, (u64, bool)> = BTreeMap::new();
    for mutant in &recording.mutants {
        if !mutant.sealed() {
            continue;
        }
        let mut said: BTreeMap<&str, SealedAnswer> = BTreeMap::new();
        for ran in executions.of(mutant) {
            let run = match ran {
                Ran::Native(_) => continue,
                Ran::Sealed(run) => run,
            };
            let target = run.target.as_str();
            let now = SealedAnswer::of(&run.came_to).ok_or(Uncounted::Unplaced {
                target,
                came_to: run.came_to.as_str(),
            })?;
            let held = said.entry(target).or_insert(now);
            *held = (*held).max(now);
        }
        for (target, answer) in said {
            let noticed = match answer {
                SealedAnswer::Detected => true,
                SealedAnswer::Passed => false,
                SealedAnswer::Doubted | SealedAnswer::SetAside => continue,
            };
            let held = asked.entry(target).or_insert((0_u64, false));
            held.0 = held
                .0
                .checked_add(1)
                .ok_or(Uncounted::Overflow { target })?;
            held.1 |= noticed;
        }
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
    for row in recording.part.faults {
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
    unattributed(recording, faulted, &mut notes);
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
    for row in recording.part.crashes {
        match crate::crashes::site(row) {
            Some(site) => reported.push(site),
            None => notes.violated("crashes", UNSHAPED_CRASH.to_owned()),
        }
    }
    crash_findings(recording, &reported, &mut notes);
    crash_accounting(recording.document, &reported, &mut notes);
    let Some(crashed) = crashed else {
        let no_site = recording
            .part
            .limitations
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
    for row in recording.part.crashes {
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

/// The dimensions a `whole-v1` run did not establish, re-derived from the flat part's records and held to its `dimension-not-measured` findings in both directions, and every column the record stream kept in `run` says, re-derived the same way (ADR 0033).
fn dimensions(recording: &Recording<'_>, run: Option<&Path>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Dimensions);
    match kept_stream(run) {
        Ok(Some(kept)) => {
            let holed = holed_dimensions(&Held::of(recording));
            for (dimension, why) in columns_disagree(&Said::of(&kept), &holed) {
                notes.violated(dimension, why);
            }
        }
        Ok(None) => {}
        Err(error) => notes.unaudited(
            "dimensions",
            format!("the record stream kept beside the report could not be read: {error}"),
        ),
    }
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
    let holed = holed_dimensions(&Held::of(recording));
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
fn schedules_holed(part: &Part<'_>) -> bool {
    let records = part.concurrency;
    let unrecorded = part.targets.iter().any(|target| {
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
fn repeatable_holed(part: &Part<'_>, knobs: &[String]) -> bool {
    let lacked = part.knobs.iter().any(|record| {
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

/// What the record stream `kept` says of each dimension: whether its one `DIMENSION` record leaves it a hole, and which dimensions its findings name.
struct Said {
    columns: Vec<(String, Result<bool, Unsaid>)>,
    named: BTreeSet<String>,
}

/// Why a `DIMENSION` record cannot say whether its column is a hole.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unsaid {
    /// A measured column without the count, or with one that is not a number.
    Count(&'static str),
    /// A measured column whose counts do not add up.
    Sum {
        catalogued: u64,
        answered: u64,
        holes: u64,
    },
    /// A state no matrix has.
    State(String),
}

impl Unsaid {
    /// What a violation says of it.
    fn said(&self) -> String {
        match self {
            Self::Count(name) => format!("a measured column with no {name} count a number holds"),
            Self::Sum {
                catalogued,
                answered,
                holes,
            } => {
                format!("{answered} answered and {holes} holes are not the {catalogued} catalogued")
            }
            Self::State(state) => format!("a column in no state a matrix has: {state}"),
        }
    }
}

impl Said {
    /// Every `DIMENSION` record and `dimension-not-measured` finding of `kept`.
    fn of(kept: &str) -> Self {
        let mut said = Self {
            columns: Vec::new(),
            named: BTreeSet::new(),
        };
        for line in kept.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                ["DIMENSION", dimension, state, rest @ ..] => {
                    said.columns
                        .push(((*dimension).to_owned(), holed_by(state, rest)));
                }
                ["FINDING", "dimension-not-measured", subject, ..] => {
                    said.named.insert((*subject).to_owned());
                }
                _ => {}
            }
        }
        said
    }
}

/// Whether a `DIMENSION` record in `state` with the counts `rest` leaves its dimension a hole, or why the record cannot say.
fn holed_by(state: &str, rest: &[&str]) -> Result<bool, Unsaid> {
    let count = |name: &'static str| -> Result<u64, Unsaid> {
        let value = rest
            .iter()
            .find_map(|field| field.strip_prefix(name))
            .ok_or(Unsaid::Count(name))?;
        value
            .parse::<u64>()
            .map_err(|_not_a_number| Unsaid::Count(name))
    };
    match state {
        "unmeasured" | "not-asked" => Ok(true),
        "nothing-to-ask" => Ok(false),
        "measured" => {
            let (catalogued, answered, holes) =
                (count("catalogued=")?, count("answered=")?, count("holes=")?);
            if answered.checked_add(holes) != Some(catalogued) {
                return Err(Unsaid::Sum {
                    catalogued,
                    answered,
                    holes,
                });
            }
            Ok(holes > 0)
        }
        other => Err(Unsaid::State(other.to_owned())),
    }
}

/// Every dimension whose `DIMENSION` records in `said` are not the one record saying what `holed` says of it, with why.
fn columns_disagree(said: &Said, holed: &BTreeSet<&'static str>) -> Vec<(&'static str, String)> {
    let mut disagree = Vec::new();
    for dimension in DIMENSIONS {
        let records: Vec<&Result<bool, Unsaid>> = said
            .columns
            .iter()
            .filter(|(name, _holed)| name == dimension)
            .map(|(_name, holed)| holed)
            .collect();
        let derived = holed.contains(dimension);
        match records.as_slice() {
            [Ok(stated)] if *stated == derived => {}
            [Ok(stated)] => disagree.push((
                dimension,
                format!(
                    "the record stream says the column {} a hole, and the records say it {}",
                    if *stated { "is" } else { "is not" },
                    if derived { "is" } else { "is not" }
                ),
            )),
            [Err(why)] => disagree.push((dimension, why.said())),
            _ => disagree.push((
                dimension,
                format!(
                    "the record stream holds {} DIMENSION record(s) for it, where a matrix has one",
                    records.len()
                ),
            )),
        }
    }
    disagree
}

/// The record stream kept in the run directory `run`, or nothing where no run directory was named or it keeps none; a symbolic link is refused rather than followed.
///
/// # Errors
/// Why a stream that is there could not be read.
pub fn kept_stream(run: Option<&Path>) -> Result<Option<String>, std::io::Error> {
    let Some(run) = run else {
        return Ok(None);
    };
    let path = run.join(LINES_FILE);
    match std::fs::symlink_metadata(&path) {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a kept record stream must not be a symbolic link",
        )),
        Ok(_file) => std::fs::read_to_string(&path).map(Some),
    }
}

/// Every dimension of the matrix, by the name a record and a finding spell it by.
pub const DIMENSIONS: [&str; 6] = [
    "mutation",
    "repeatable",
    "fault",
    "schedule",
    "wire",
    "durable",
];

/// What a matrix is re-derived from: the lists of a flat part, or of every part of a merge, whether some mutation was put and not decided, and each finding's kind and subject.
struct Held<'a> {
    part: Part<'a>,
    unsettled: bool,
    findings: Vec<(&'a str, &'a str)>,
}

/// Whether a mutation that came to `outcome` was put and not decided: an outcome no run decides, or a lead no sealed execution established.
fn undecided(outcome: &str, lead: bool) -> bool {
    lead || [
        "waited",
        "step-limit-reached",
        "unconfirmed",
        "errored",
        "declined",
    ]
    .contains(&outcome)
}

impl<'a> Held<'a> {
    /// What the flat part of `recording` holds.
    fn of(recording: &'a Recording<'a>) -> Self {
        Self {
            part: recording.part,
            unsettled: recording
                .mutants
                .iter()
                .any(|mutant| undecided(&mutant.outcome, mutant.lead())),
            findings: recording
                .findings
                .iter()
                .map(|finding| (finding.kind.as_str(), finding.subject.as_str()))
                .collect(),
        }
    }
}

/// Whether a mutation was put and not decided, or a place was passed over by a choice another run could make otherwise.
fn mutations_holed(held: &Held<'_>) -> bool {
    held.unsettled
        || held.part.limitations.iter().any(|row| {
            field(row, "name").is_some_and(|name| {
                [
                    "skipped-excluded",
                    "skipped-annotated",
                    "skipped-configured",
                ]
                .contains(&name.as_str())
            })
        })
}

/// Every dimension the records `held` leave a hole, by name, read without any of the runner's code.
fn holed_dimensions(held: &Held<'_>) -> BTreeSet<&'static str> {
    let part = &held.part;
    let state = |rows: &[serde_json::Value], field: &str| -> Vec<String> {
        rows.iter()
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
        part.limitations
            .iter()
            .any(|row| field(row, "name").as_deref() == Some(name))
    };
    let mut holed = BTreeSet::new();
    if held
        .findings
        .iter()
        .any(|(kind, _subject)| *kind == "build-failure")
    {
        holed.extend(DIMENSIONS);
        return holed;
    }
    if schedules_holed(part) {
        holed.insert("schedule");
    }
    if mutations_holed(held) {
        holed.insert("mutation");
    }
    if repeatable_holed(part, &state(part.knobs, "standing")) {
        holed.insert("repeatable");
    }
    let faults = state(part.faults, "decision");
    let unmeasured = held
        .findings
        .iter()
        .any(|(_kind, subject)| *subject == "fault-baseline-not-measured");
    if unmeasured
        || (faults.is_empty() && !limited("fault-no-site"))
        || none_but_not_put(&faults)
        || faults
            .iter()
            .any(|one| one == "waited" || one == "undecided")
    {
        holed.insert("fault");
    }
    let crashes = state(part.crashes, "decision");
    let crashed_unmeasured = held
        .findings
        .iter()
        .any(|(_kind, subject)| *subject == "crash-baseline-not-measured");
    if crashed_unmeasured
        || (crashes.is_empty() && !limited("crash-no-site"))
        || none_but_not_put(&crashes)
        || crashes
            .iter()
            .any(|one| one == "unshared" || one == "undecided")
    {
        holed.insert("durable");
    }
    let seams = state(part.seams, "answer");
    if limited("seam-not-watched") || seams.iter().any(|one| one == "unreached") {
        holed.insert("wire");
    }
    holed
}

/// Every survivor's evidence beside a fault the report holds, each that is not the shape a run writes a violation.
fn besides_of(recording: &Recording<'_>, notes: &mut Notes<'_>) -> Vec<crate::faults::Beside> {
    let mut held: Vec<crate::faults::Beside> = Vec::new();
    for row in recording.part.beside {
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

/// The fault counts, re-derived from the report's own records, since a part whose seven decisions do not add up to its sites is refused whatever the recording says.
fn fault_counts(
    recording: &Recording<'_>,
    reported: &[crate::faults::Site],
    notes: &mut Notes<'_>,
) {
    for (column_name, decision) in [
        ("noticed", "noticed"),
        ("unnoticed", "unnoticed"),
        ("absorbed", "absorbed"),
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

/// The `fault-write-unattributed` finding, owed wherever a path the phase left written was put to a fault run alone and no such run tied it, and naming every such path, and no path a run tied.
fn unattributed(
    recording: &Recording<'_>,
    faulted: &crate::faults::Faulted,
    notes: &mut Notes<'_>,
) {
    let tied: BTreeSet<&str> = faulted
        .writes
        .iter()
        .filter(|(_, tied)| *tied)
        .map(|(path, _)| path.as_str())
        .collect();
    let untied: BTreeSet<&str> = faulted
        .writes
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| !tied.contains(path))
        .collect();
    let details: Vec<Option<String>> = recording
        .part
        .findings
        .iter()
        .filter(|row| {
            field(row, "kind").as_deref() == Some(NOT_MEASURED_FINDING)
                && field(row, "subject").as_deref() == Some(UNATTRIBUTED_WRITE)
        })
        .map(|row| field(row, "detail"))
        .collect();
    match details.as_slice() {
        [] if untied.is_empty() => {}
        [] => notes.violated(
            UNATTRIBUTED_WRITE,
            format!(
                "the recording puts {} to a fault run alone and ties none of them, and the report \
                 raises no {UNATTRIBUTED_WRITE} finding",
                untied.iter().copied().collect::<Vec<&str>>().join(", ")
            ),
        ),
        [None] => notes.violated(
            UNATTRIBUTED_WRITE,
            "the finding names no path it is about".to_owned(),
        ),
        [Some(detail)] => {
            for path in untied.iter().filter(|path| !detail.contains(**path)) {
                notes.violated(
                    UNATTRIBUTED_WRITE,
                    format!("no fault run alone tied {path}, and the finding does not name it"),
                );
            }
            for path in tied.iter().filter(|path| detail.contains(**path)) {
                notes.violated(
                    UNATTRIBUTED_WRITE,
                    format!("a fault run alone tied {path}, and the finding calls it unattributed"),
                );
            }
        }
        several => notes.violated(
            UNATTRIBUTED_WRITE,
            format!(
                "the report raises {} {UNATTRIBUTED_WRITE} findings where one names every path",
                several.len()
            ),
        ),
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
                "unnoticed" | "absorbed" => UNNOTICED_FAULT,
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

/// Each kill, wait and unconfirmed disposition the report states, held to the last confirmation the recording holds for it against its target and to what that confirmation decides.
/// One disposition read back from an earlier run or inherited from an interrupted one was confirmed there, not here.
fn confirmations(
    recording: &Recording<'_>,
    confirmed: Option<&crate::confirm::Confirmations>,
    audit: &mut Audit,
) -> Decided {
    use crate::confirm::{ConfirmRule, Expected};
    let mut notes = Notes::on(audit, Layer::Confirmations);
    let mutants: Vec<&MutantRow> = recording
        .mutants
        .iter()
        .filter(|mutant| {
            mutant.read_back_from.is_none()
                && matches!(mutant.outcome.as_str(), KILLED | WAITED | UNCONFIRMED)
        })
        .collect();
    if mutants.is_empty() {
        return notes.absent("this run reports no new kill, wait or unconfirmed disposition");
    }
    let asked: BTreeSet<&str> = match confirmed {
        Some(confirmed) => confirmed
            .confirms
            .iter()
            .map(|(_, confirm)| confirm.mutant.as_str())
            .collect(),
        None => BTreeSet::new(),
    };
    let mutants: Vec<&MutantRow> = mutants
        .into_iter()
        .filter(|mutant| !mutant.sealed() || asked.contains(mutant.id.as_str()))
        .collect();
    if mutants.is_empty() {
        return notes.absent(
            "every new kill this run reports rests on sealed executions, which owe no native \
             confirmation, and the recording confirms none of them natively",
        );
    }
    let Some(confirmed) = confirmed else {
        notes.unaudited(
            "confirmations",
            "the run kept no recording, so how its kills and waits were confirmed cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    let broke = |notes: &mut Notes<'_>, subject: &str, rule: ConfirmRule, why: &str| {
        notes.violated(subject, format!("{}: {why}", rule.label()));
    };
    for (target, test) in confirmed.asked_twice() {
        let target = match target.as_deref() {
            Some(target) => target,
            None => "the whole suite",
        };
        let test = match test.as_deref() {
            Some(test) => test,
            None => "with every test",
        };
        broke(
            &mut notes,
            "control",
            ConfirmRule::Twice,
            &format!(
                "the original code was asked about {target} {test} more than once, which one control per question rules out"
            ),
        );
    }
    for mutant in mutants {
        let owed = match mutant.outcome.as_str() {
            KILLED => Some(Expected::Killed),
            WAITED => Some(Expected::Waited),
            _ => None,
        };
        if confirmed.resumed.contains(&mutant.id) {
            notes.unaudited(
                mutant.label(),
                "it was inherited from an interrupted run's checkpoint, so it was confirmed in that run's recording and not in this one"
                    .to_owned(),
            );
            continue;
        }
        confirmation_of(mutant, owed, confirmed, &mut notes);
    }
    notes.looked()
}

/// One disposition, owed a standing confirmation where `owed` names what it stands as and a failed one where it names nothing, held to the confirmations `confirmed` holds of it against its target.
fn confirmation_of(
    mutant: &MutantRow,
    owed: Option<crate::confirm::Expected>,
    confirmed: &crate::confirm::Confirmations,
    notes: &mut Notes<'_>,
) {
    use crate::confirm::{ConfirmRule, Decided};
    let mut broke = |rule: ConfirmRule, why: &str| {
        notes.violated(mutant.label(), format!("{}: {why}", rule.label()));
    };
    let on: Vec<Decided> = confirmed
        .confirms
        .iter()
        .filter(|(_, confirm)| {
            confirm.mutant == mutant.id
                && confirm.target.as_deref() == mutant.killed_by.as_deref()
                && owed.is_none_or(|expected| confirm.expected == expected)
        })
        .map(|(seq, confirm)| confirmed.decided(*seq, confirm))
        .collect();
    for decided in &on {
        if let Decided::Broke(rule, why) = decided {
            broke(*rule, why);
        }
    }
    let target = match mutant.killed_by.as_deref() {
        Some(target) => target,
        None => "no target",
    };
    match (owed, on.last()) {
        (_, None) => broke(
            ConfirmRule::Missing,
            &format!(
                "the report says {} against {}, and the recording holds no confirmation of it there, so nothing a reader can check says the original code passed that test and the result came back",
                mutant.outcome, target
            ),
        ),
        (Some(_), Some(Decided::Unconfirmed)) => broke(
            ConfirmRule::Unconfirmed,
            &format!(
                "the report says {}, and its last confirmation leaves it unconfirmed",
                mutant.outcome
            ),
        ),
        (None, Some(Decided::Stands)) => broke(
            ConfirmRule::Confirmed,
            "the report says unconfirmed, and its last confirmation passed its control and came back",
        ),
        (Some(_), Some(Decided::Stands | Decided::Broke(..)))
        | (None, Some(Decided::Unconfirmed | Decided::Broke(..))) => {}
    }
}

/// What the engine calls a step-limit outcome in the executions it records.
const STEP_LIMIT_EXEC: &str = "step_limit_reached";

/// Every mutation's reported outcome against the executions of it the recording holds, so a report cannot say a test noticed what every recorded execution says survived, nor rest on a sealed execution its engine never ran.
fn executions_held(
    recording: &Recording<'_>,
    (routing, executions, engines): (Option<&crate::route::Routing>, &Executions<'_>, &[Engine]),
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
    match executions.sealed {
        Kept::Held => {}
        Kept::Unreadable => {
            notes.unaudited(
                "sealed-exec",
                "an engine recording holds a sealed execution this audit cannot read, so no \
                 sealed verdict can be held to what ran"
                    .to_owned(),
            );
            return notes.looked();
        }
        Kept::Unrecorded => {
            if recording.mutants.iter().any(MutantRow::sealed) {
                notes.unaudited(
                    "sealed-exec",
                    "the run kept no engine recording, so no sealed verdict can be held to the \
                     executions its engine ran"
                        .to_owned(),
                );
            }
        }
    }
    for mutant in recording
        .mutants
        .iter()
        .filter(|mutant| mutant.read_back_from.is_none())
    {
        let mut recorded: Vec<&str> = Vec::new();
        let mut sealed: Vec<&SealedRun> = Vec::new();
        for ran in executions.of(mutant) {
            match ran {
                Ran::Native(exec) => recorded.push(exec.outcome.as_str()),
                Ran::Sealed(run) => sealed.push(run),
            }
        }
        let sealed_runs = (executions.sealed == Kept::Held).then_some(sealed);
        if let Some(why) = misexecuted(mutant, &recorded, sealed_runs) {
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
    if executions.sealed == Kept::Held {
        sealed_unreached(recording, (routing, engines), &mut notes);
    }
    notes.looked()
}

/// Every row sealed executions call unreached, held to the sealed controls its engines recorded as the engine decides it (ADR 0046): no control reached its guard, and every target its route reaches it through natively has a station every control of which the sealed build answered for.
fn sealed_unreached(
    recording: &Recording<'_>,
    (routing, engines): (&crate::route::Routing, &[Engine]),
    notes: &mut Notes<'_>,
) {
    let mut controls: Vec<&crate::route::SealedControl> = Vec::new();
    for engine in engines {
        match &engine.controls {
            Some(recorded) => controls.extend(recorded.iter()),
            None => {
                notes.unaudited(
                    "sealed-control",
                    "an engine recording holds a sealed control this audit cannot read, so no \
                     sealed unreached verdict can be held to what its controls reached"
                        .to_owned(),
                );
                return;
            }
        }
    }
    for mutant in recording.mutants.iter().filter(|mutant| {
        mutant.outcome == UNREACHED
            && matches!(&mutant.rests, Rests::Sealed(named) if named.is_empty())
    }) {
        let Some(index) = mutant.catalog_index else {
            notes.violated(
                mutant.label(),
                "the report calls this mutation unreached on sealed evidence and names no catalog \
                 index, so no sealed control can be held to it"
                    .to_owned(),
            );
            continue;
        };
        if let Some(control) = controls
            .iter()
            .find(|control| control.reached.contains(&index))
        {
            notes.violated(
                mutant.label(),
                format!(
                    "the report says no sealed test reaches this mutation, and the sealed control \
                     of {} {} reached its guard",
                    control.target, control.test
                ),
            );
            continue;
        }
        let Some(route) = routing.route_of(&mutant.id, &mutant.display_id) else {
            continue;
        };
        for target in &route.reaching {
            let held: Vec<&&crate::route::SealedControl> = controls
                .iter()
                .filter(|control| control.target == *target)
                .collect();
            if held.is_empty() {
                notes.violated(
                    mutant.label(),
                    format!(
                        "the report says no sealed test reaches this mutation, its route says \
                         {target} reaches it natively, and no engine recording holds a sealed \
                         control of {target}"
                    ),
                );
            } else if let Some(uncontrolled) = held
                .iter()
                .find(|control| control.standing != SEALED_CONTROLLED)
            {
                notes.violated(
                    mutant.label(),
                    format!(
                        "the report says no sealed test reaches this mutation, and {target}'s \
                         test {} has no control ({})",
                        uncontrolled.test, uncontrolled.standing
                    ),
                );
            }
        }
    }
}

/// What a sealed control's standing is where a mutant's execution can be judged against it.
const SEALED_CONTROLLED: &str = "controlled";

/// Why the executions of `mutant` do not bear out its outcome: the native ones `recorded`, and the sealed ones its engine recorded where the run kept an engine recording.
fn misexecuted(
    mutant: &MutantRow,
    recorded: &[&str],
    sealed: Option<Vec<&SealedRun>>,
) -> Option<String> {
    match (&mutant.rests, mutant.sealed()) {
        (Rests::Sealed(named), true) => {
            if let Some(why) = sealed.and_then(|ran| unrecorded_sealed(named, ran)) {
                return Some(why);
            }
            if recorded.is_empty() {
                None
            } else {
                contradicted(&mutant.outcome, recorded)
            }
        }
        (Rests::Nothing | Rests::Sealed(_) | Rests::Unproven(_), _) => {
            contradicted(&mutant.outcome, recorded)
        }
    }
}

/// Why the sealed executions a row `named` are not the ones its engine `recorded`, in order, if they are not.
fn unrecorded_sealed(named: &[SealedRun], recorded: Vec<&SealedRun>) -> Option<String> {
    let said = |runs: &mut dyn Iterator<Item = &SealedRun>| {
        runs.map(|run| format!("{} {} {}", run.target, run.test, run.came_to))
            .collect::<Vec<_>>()
    };
    (!named.iter().eq(recorded.iter().copied())).then(|| {
        format!(
            "the report rests on the sealed executions {:?}, and its engine recorded {:?}",
            said(&mut named.iter()),
            said(&mut recorded.into_iter())
        )
    })
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
    rerouted: Option<Rerouted<'_>>,
    executions: &Executions<'_>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let Some(Rerouted { routing, repairs }) = rerouted else {
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
    let mut executed = Executed::default();
    let mut noticed: Vec<Noticed<'_>> = Vec::new();
    for (mutant, ran) in executions.each() {
        let noticing = match ran {
            Ran::Native(exec) => {
                if repairs
                    .iter()
                    .any(|repair| repair.mutant == exec.mutant && repair.target == exec.target)
                {
                    continue;
                }
                executed.natively.insert(mutant);
                exec.outcome == KILLED
            }
            Ran::Sealed(run) => DETECTIONS.contains(&run.came_to.as_str()),
        };
        executed.at_all.insert(mutant);
        if noticing {
            noticed.push(Noticed { mutant, by: ran });
        }
    }
    if removed.is_empty() && executed.natively.is_empty() {
        notes.unaudited(
            "route",
            "the recording holds no routing decision and no native mutation execution, so \
             there is nothing to hold a layer to; a sealed execution is put where its own \
             control reached, which no route decides"
                .to_owned(),
        );
        return notes.looked();
    }
    let known: BTreeSet<&str> = recording
        .targets
        .iter()
        .map(|target| target.name.as_str())
        .collect();
    discharges(&removed, &noticed, &mut notes);
    kept(&routing.routes, &noticed, &mut notes);
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

/// The mutations the proofs layer holds a route to having run: natively, where a route's claim about the native measurement is contradicted, and at all, where its premise failing has to end in work.
#[derive(Debug, Default)]
struct Executed<'a> {
    natively: BTreeSet<&'a str>,
    at_all: BTreeSet<&'a str>,
}

/// One execution that noticed a mutation: a native kill, which is a lead, or a sealed detection, which is a kill the run proved (ADR 0046).
#[derive(Debug, Clone, Copy)]
struct Noticed<'a> {
    mutant: &'a str,
    by: Ran<'a>,
}

impl Noticed<'_> {
    /// What its target did, as a violation says it.
    fn said(&self) -> String {
        match self.by {
            Ran::Native(_) => format!("{KILLED} it"),
            Ran::Sealed(run) => format!(
                "detected it: its sealed execution of {} came to {}",
                run.test, run.came_to
            ),
        }
    }
}

/// Every proof that removed a target, against every execution that noticed the mutation it removed the target from, sealed or native: a layer that drops a target which then finds a defect is unsound.
fn discharges(
    removed: &BTreeMap<String, BTreeMap<String, String>>,
    noticed: &[Noticed<'_>],
    notes: &mut Notes<'_>,
) {
    for one in noticed {
        let target = one.by.target();
        let Some(proof) = removed.get(one.mutant).and_then(|by| by.get(target)) else {
            continue;
        };
        notes.violated(
            one.mutant,
            format!(
                "{proof} removed {target} from what could notice this mutation, and {target} \
                 then {}; a layer that drops a target which finds a defect is unsound",
                one.said()
            ),
        );
    }
}

/// Every native kill, against the route that decided which targets would be asked: a layer that drops a target which then finds a defect is unsound, however it dropped it.
///
/// A sealed detection is not held here: a sealed execution is put to the tests whose own control reached the mutation, which the native route does not decide.
fn kept(routes: &[crate::route::Route], noticed: &[Noticed<'_>], notes: &mut Notes<'_>) {
    for one in noticed {
        match one.by {
            Ran::Native(_) => {}
            Ran::Sealed(_) => continue,
        }
        let (mutant, target) = (one.mutant, one.by.target());
        let Some(route) = routes
            .iter()
            .find(|route| route.mutant == mutant && route.reused.is_none())
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
                 shows {target} {}; a target the measurement placed elsewhere is \
                 a target the reach layer removed, and a layer that removes one which \
                 finds a defect is unsound",
                one.said()
            ),
        );
    }
}

/// The reach layer, re-derived from what the route named rather than confirmed from what it decided.
///
/// A route that says no measured target reaches a mutation is contradicted by a native execution of it; a sealed one is put where its own control reached, which is another measurement.
/// A route widened to every target is held to anything running at all.
fn reach(
    routes: &[crate::route::Route],
    known: &BTreeSet<&str>,
    executed: &Executed<'_>,
    notes: &mut Notes<'_>,
) {
    for route in routes {
        if route.reused.is_some() {
            continue;
        }
        let ran = executed.at_all.contains(route.mutant.as_str());
        if route.granularity == UNREACHED {
            if executed.natively.contains(route.mutant.as_str()) {
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
    /// The run the whole report was read back from, where it was; a report this run established names none.
    restated_from: Option<String>,
    targets: Vec<TargetRow>,
    mutants: Vec<MutantRow>,
    findings: Vec<FindingRow>,
    shard: Option<String>,
    target: String,
    models: Vec<ModelRow>,
    /// What the report says of model checking.
    modeled: Modeled,
    /// The lists of the report's part the layers read as they stand.
    part: Part<'a>,
    /// What the run concluded, as its recording's `run-end` says; a complete report stores no verdict.
    verdict: Option<String>,
}

/// The lists of the report's one part the layers read as they stand, each one its schema requires.
#[derive(Debug, Clone, Copy)]
struct Part<'a> {
    targets: &'a [serde_json::Value],
    findings: &'a [serde_json::Value],
    limitations: &'a [serde_json::Value],
    drift: &'a [serde_json::Value],
    repaired: &'a [serde_json::Value],
    knobs: &'a [serde_json::Value],
    concurrency: &'a [serde_json::Value],
    faults: &'a [serde_json::Value],
    beside: &'a [serde_json::Value],
    crashes: &'a [serde_json::Value],
    seams: &'a [serde_json::Value],
}

impl<'a> Part<'a> {
    /// Every list of `document`, a flat part.
    ///
    /// # Errors
    /// [`crate::route::ReadCauseError::Absent`] for the first list that is not there or is not an array.
    fn of(document: &'a serde_json::Value) -> Result<Self, crate::route::ReadCauseError> {
        Ok(Self {
            targets: rows(document, "targets")?,
            findings: rows(document, "findings")?,
            limitations: rows(document, "limitations")?,
            drift: rows(document, "drift")?,
            repaired: rows(document, "repaired")?,
            knobs: rows(document, "knobs")?,
            concurrency: rows(document, "concurrency")?,
            faults: rows(document, "faults")?,
            beside: rows(document, "beside")?,
            crashes: rows(document, "crashes")?,
            seams: rows(document, "seams")?,
        })
    }
}

/// The run the whole report was read back from, where it was, as its provenance names it; a report established by this run names none.
fn restated_from(
    document: &serde_json::Value,
) -> Result<Option<String>, crate::route::ReadCauseError> {
    crate::route::required(
        crate::route::required(document, "provenance", Some)?,
        "source_run_id",
        |said: &serde_json::Value| Some(said.as_str().map(str::to_owned)),
    )
}

impl<'a> Recording<'a> {
    /// The rows every layer reads, each field its schema requires demanded rather than supplied.
    fn of(
        document: &'a serde_json::Value,
        modeled: Modeled,
    ) -> Result<Self, crate::route::ReadCauseError> {
        use crate::route::required;
        let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
        let restated_from = restated_from(document)?;
        let part = Part::of(document)?;
        let targets = part
            .targets
            .iter()
            .map(|row| {
                Ok(TargetRow {
                    id: required(row, "id", text)?,
                    name: required(row, "name", text)?,
                    status: required(row, "status", text)?,
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let mutants = rows(document, "mutants")?
            .iter()
            .map(|row| {
                let decision = required(row, "decision", Some)?;
                Ok(MutantRow {
                    id: required(row, "id", text)?,
                    display_id: required(row, "display_id", text)?,
                    catalog_index: row.get("catalog_index").and_then(serde_json::Value::as_u64),
                    edit: RowEdit::read(row).ok_or_else(|| {
                        crate::route::ReadCauseError::Absent {
                            field: "position".to_owned(),
                        }
                    })?,
                    outcome: required(decision, "outcome", text)?,
                    acceptance: AcceptanceFact::from_json(row.get("accepted")),
                    killed_by: field(decision, "killed_by"),
                    read_back_from: row
                        .get("reuse")
                        .and_then(|reuse| field(reuse, "source_run_id")),
                    rests: required(row, "evidence", Rests::read)?,
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let findings = part
            .findings
            .iter()
            .map(|row| {
                Ok(FindingRow {
                    kind: required(row, "kind", text)?,
                    subject: required(row, "subject", text)?,
                })
            })
            .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()?;
        let models = rows(document, "models")?
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
            restated_from,
            targets,
            mutants,
            findings,
            shard: document
                .get("scope")
                .and_then(|scope| field(scope, "shard")),
            target: required(required(document, "toolchain", Some)?, "target", text)?,
            models,
            modeled,
            part,
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
        && !(part && unsettled(&recording.part));
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
fn unsettled(part: &Part<'_>) -> bool {
    part.drift
        .iter()
        .any(|row| field(row, "state").as_deref() == Some("moved"))
        || part.knobs.iter().any(|row| {
            matches!(
                row.get("standing")
                    .and_then(|standing| field(standing, "state"))
                    .as_deref(),
                Some("broke" | "moved")
            )
        })
}

/// Whether a mutation row is an answer: decided by a test, a model or the compiler, or accepted as it stands, and never a lead, which no sealed execution decided.
fn answers(mutant: &MutantRow) -> bool {
    !mutant.lead()
        && matches!(
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
        if mutant.lead() {
            unsupported(
                &mut notes,
                &format!(
                    "mutation {} ended as {} on no sealed execution, which is a lead and no \
                     answer",
                    mutant.label(),
                    mutant.outcome
                ),
            );
        } else if !answers(mutant) {
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
        UNPROVEN_MUTANT,
        WAITED_MUTANT,
        STEP_LIMIT_REACHED_MUTANT,
        FAILING_TEST,
        TARGET_MISSING,
        NOT_MEASURED,
    ];
    for mutant in &recording.mutants {
        let expected = match (mutant.outcome.as_str(), mutant.acceptance) {
            (_, AcceptanceFact::Missing) => continue,
            (_, AcceptanceFact::Rejected | AcceptanceFact::Accepted) if mutant.lead() => {
                Some(UNPROVEN_MUTANT)
            }
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
            SURVIVING_MUTANT | UNPROVEN_MUTANT | WAITED_MUTANT | STEP_LIMIT_REACHED_MUTANT
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
/// A carried answer is held to every premise of ADR 0041 again, from what its build kept beside its recording.
/// Whether each target an exact answer rests on keeps the behaviour key it had is not in the report, and is left unaudited.
fn reuse(
    recording: &Recording<'_>,
    (routing, (beside, root), engines, executions): Reusing<'_>,
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
    let (restated, believed): (Vec<&MutantRow>, Vec<&MutantRow>) = read_back
        .into_iter()
        .partition(|mutant| mutant.read_back_from == recording.restated_from);
    if let (false, Some(source)) = (restated.is_empty(), recording.restated_from.as_deref()) {
        notes.unaudited(
            "provenance",
            format!(
                "{} dispositions were restated with the whole report read back from {source}, \
                 which this audit was not given, so nothing this run recorded holds them but the \
                 sealed executions it ran again",
                restated.len()
            ),
        );
        for mutant in &restated {
            let again: Vec<&SealedRun> = executions
                .of(mutant)
                .into_iter()
                .filter_map(|ran| match ran {
                    Ran::Native(_) => None,
                    Ran::Sealed(run) => Some(run),
                })
                .collect();
            reproduced(mutant, &again, executions.sealed, &mut notes);
        }
    }
    match routing {
        Some(routing) => {
            for mutant in &believed {
                routed_back(mutant, routing, executions, &mut notes);
            }
            carried_back(&believed, routing, (beside, engines), &mut notes);
            bodies_read_again(&believed, routing, (beside, root), &mut notes);
        }
        None => notes.unaudited(
            "provenance",
            format!(
                "{} dispositions were read back from an earlier run, and the run kept no \
                 recording of the routes it read them back under",
                believed.len()
            ),
        ),
    }
    notes.unaudited(
        "provenance",
        format!(
            "{} dispositions were read back from an earlier run; whether each target an exact \
             answer rests on keeps the behaviour key it had is a fact this report does not carry",
            believed.len()
        ),
    );
    notes.looked()
}

/// Whether every body an answer the run carried rests on is, in the tree `root` names, the body the build kept the digest and start of, in a file the skeletons' digest proves the one measured; unaudited where no `--root` names the tree.
fn bodies_read_again(
    read_back: &[&MutantRow],
    routing: &crate::route::Routing,
    (beside, root): (&[Beside], Option<&Path>),
    notes: &mut Notes<'_>,
) {
    let carried = read_back.iter().any(|mutant| {
        routing
            .route_of(&mutant.id, &mutant.display_id)
            .is_some_and(|route| route.rule.as_deref() == Some("carried"))
    });
    if !carried || beside.is_empty() {
        return;
    }
    let Some(root) = root else {
        notes.unaudited(
            "root",
            "no --root names the tree the run measured, so the bodies its carried answers rest on \
             are held to the digests the run kept of them, not read again"
                .to_owned(),
        );
        return;
    };
    for kept in beside {
        let said = crate::engineaudit::carry::bodies_again(
            crate::engineaudit::carry::Kept {
                carried: &kept.carried,
                skeletons: &kept.skeletons,
                touched: &kept.touched,
                catalog: &kept.catalog,
            },
            root,
        );
        match said {
            Ok(said) => {
                for one in said {
                    match one {
                        crate::engineaudit::carry::ReadAgain::Violated { subject, detail } => {
                            notes.violated(&subject, detail);
                        }
                        crate::engineaudit::carry::ReadAgain::Unaudited { subject, detail } => {
                            notes.unaudited(&subject, detail);
                        }
                    }
                }
            }
            Err(why) => notes.violated(&kept.path, crate::error::Coded::coded(&why)),
        }
    }
}

/// What the reuse layer reads beside the report: the run's routes, what each build kept of the answers it carried with the tree they are read again from, each build's engine recording, and the executions the recording holds.
type Reusing<'a> = (
    Option<&'a crate::route::Routing>,
    (&'a [Beside], Option<&'a Path>),
    &'a [Engine],
    &'a Executions<'a>,
);

/// What each row read back says of its mutation's edit and catalog index, by identity, for a row that names its index.
fn reported_of(read_back: &[&MutantRow]) -> BTreeMap<String, crate::engineaudit::carry::Reported> {
    read_back
        .iter()
        .filter_map(|mutant| {
            Some((
                mutant.id.clone(),
                crate::engineaudit::carry::Reported {
                    index: mutant.catalog_index?,
                    path: mutant.edit.path.clone(),
                    line: mutant.edit.line,
                    column: mutant.edit.column,
                    original: mutant.edit.original.clone(),
                    replacement: mutant.edit.replacement.clone(),
                },
            ))
        })
        .collect()
}

/// Whether every disposition the run's own routes say was carried from an earlier tree rests on a record a build kept beside its recording, says what the report says, and meets every premise of ADR 0041, re-derived from those documents and that build's control records.
fn carried_back(
    read_back: &[&MutantRow],
    routing: &crate::route::Routing,
    (beside, engines): (&[Beside], &[Engine]),
    notes: &mut Notes<'_>,
) {
    let reported = reported_of(read_back);
    let mut believed: BTreeMap<String, crate::engineaudit::carry::Rederived> = BTreeMap::new();
    for kept in beside {
        let standings = engines
            .get(kept.engine)
            .map(|engine| crate::drift::standings(&engine.touched));
        match crate::engineaudit::carry::rederived(
            crate::engineaudit::carry::Kept {
                carried: &kept.carried,
                skeletons: &kept.skeletons,
                touched: &kept.touched,
                catalog: &kept.catalog,
            },
            (standings.as_ref(), &reported),
        ) {
            Ok(records) => {
                believed.extend(records.into_iter().map(|one| (one.mutant.clone(), one)));
            }
            Err(why) => notes.violated(&kept.path, crate::error::Coded::coded(&why)),
        }
    }
    for mutant in read_back {
        let carried = routing
            .route_of(&mutant.id, &mutant.display_id)
            .is_some_and(|route| route.rule.as_deref() == Some("carried"));
        if !carried {
            continue;
        }
        let Some(record) = believed.get(&mutant.id) else {
            notes.violated(
                mutant.label(),
                "the disposition was carried from an earlier tree, and no build kept the record \
                 it rests on beside its recording, so no premise of ADR 0041 can be read again"
                    .to_owned(),
            );
            continue;
        };
        if record.outcome != mutant.outcome
            || Some(record.run_id.as_str()) != mutant.read_back_from.as_deref()
        {
            notes.violated(
                mutant.label(),
                format!(
                    "the report says {} from {:?}, and the record it carried says {} from {}",
                    mutant.outcome, mutant.read_back_from, record.outcome, record.run_id
                ),
            );
        }
        if let Some(why) = &record.misplaced {
            notes.violated(
                mutant.label(),
                format!("the record's locus is not the mutation's: {why}"),
            );
        }
        if let Some(why) = &record.fails {
            notes.violated(
                mutant.label(),
                format!("the run carried it though a premise of ADR 0041 fails: {why}"),
            );
        }
        for why in &record.unplanned {
            notes.violated(
                mutant.label(),
                format!("the run held it to a plan the guards' record does not bear out: {why}"),
            );
        }
        for why in &record.unkept {
            notes.unaudited(mutant.label(), why.clone());
        }
    }
}

/// Whether the run's own route of `mutant`, a disposition read back, says it read it back from the run the report names, ran none of it, and still reaches the target a kill names.
fn routed_back(
    mutant: &MutantRow,
    routing: &crate::route::Routing,
    executions: &Executions<'_>,
    notes: &mut Notes<'_>,
) {
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
    let mut native = false;
    let mut again: Vec<&SealedRun> = Vec::new();
    for ran in executions.of(mutant) {
        match ran {
            Ran::Native(_) => native = true,
            Ran::Sealed(run) => again.push(run),
        }
    }
    if native {
        notes.violated(
            mutant.label(),
            "an answer read back is an execution that did not happen, and the recording holds \
             an execution of it"
                .to_owned(),
        );
    }
    reproduced(mutant, &again, executions.sealed, notes);
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

/// Whether a sealed verdict read back was believed only once the executions it rests on, `again` on this run's bench, came to what it recorded (ADR 0046, decision 7); a lead read back rests on nothing sealed, so nothing of it is run again.
fn reproduced(mutant: &MutantRow, again: &[&SealedRun], kept: Kept, notes: &mut Notes<'_>) {
    let named = match (&mutant.rests, mutant.sealed()) {
        (Rests::Sealed(named), true) => named,
        (Rests::Nothing | Rests::Unproven(_) | Rests::Sealed(_), _) => return,
    };
    match kept {
        Kept::Held => {}
        Kept::Unrecorded | Kept::Unreadable => {
            notes.unaudited(
                mutant.label(),
                "a sealed verdict was read back, and the run kept no engine recording this audit \
                 can read of its executions made again, so whether they came out the same \
                 cannot be re-derived"
                    .to_owned(),
            );
            return;
        }
    }
    if !named.iter().eq(again.iter().copied()) {
        let said = |runs: &mut dyn Iterator<Item = &SealedRun>| {
            runs.map(|run| format!("{} {} {}", run.target, run.test, run.came_to))
                .collect::<Vec<_>>()
        };
        notes.violated(
            mutant.label(),
            format!(
                "the verdict read back from {} rests on the sealed executions {:?}, and this \
                 run's engine recorded {:?} of it; a kept sealed verdict is believed only once \
                 its executions come out the same on this run's bench",
                match mutant.read_back_from.as_deref() {
                    Some(run) => run,
                    None => "an earlier run",
                },
                said(&mut named.iter()),
                said(&mut again.iter().copied())
            ),
        );
    }
}

/// The columns of the accounting, against the records they summarise and against the verdict they carry.
/// Whether every row rests on what its outcome can rest on, decided again from the evidence it names, with no lead accepted and every lead counted (ADR 0046).
fn evidence(recording: &Recording<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Evidence);
    for mutant in &recording.mutants {
        if let Some(why) = misrested(mutant) {
            notes.violated(mutant.label(), why);
        }
        if mutant.lead() && mutant.acceptance == AcceptanceFact::Accepted {
            notes.violated(
                mutant.label(),
                format!(
                    "a lead is accepted as {}, and an acceptance answers what the tests were \
                     asked, which a lead says nothing of",
                    mutant.outcome
                ),
            );
        }
    }
    let leads = recording
        .mutants
        .iter()
        .filter(|mutant| mutant.lead())
        .count();
    notes.tally(Column {
        subject: "accounting.mutants.observers.unproven",
        recorded: recording
            .document
            .get("accounting")
            .and_then(|accounting| accounting.get("mutants"))
            .and_then(|mutants| mutants.get("observers"))
            .and_then(|observers| observers.get("unproven"))
            .and_then(serde_json::Value::as_u64),
        derived: size(leads),
        records: "the rows no sealed execution decided",
    });
    notes.looked()
}

/// Why what `mutant` rests on cannot be what its outcome rests on, or nothing where it can.
fn misrested(mutant: &MutantRow) -> Option<String> {
    let outcome = mutant.outcome.as_str();
    match (&mutant.rests, outcome) {
        (Rests::Nothing, REJECTED) => None,
        (Rests::Nothing, _) => Some(format!(
            "outcome {outcome} rests on executions and the row names none"
        )),
        (Rests::Sealed(_) | Rests::Unproven(_), REJECTED) => Some(
            "the compiler refused the mutation, so no execution was asked about it, and the \
             row names evidence"
                .to_owned(),
        ),
        (Rests::Unproven(reasons), _) => unreasoned(reasons),
        (Rests::Sealed(executions), _) => {
            sealed_against(executions, outcome, mutant.killed_by.as_deref())
        }
    }
}

/// Why `reasons` are not every reason a sealed run gives for no verdict, or nothing where they are.
fn unreasoned(reasons: &[String]) -> Option<String> {
    if reasons.is_empty() {
        return Some(
            "a row no sealed execution decided names no reason it has no verdict".to_owned(),
        );
    }
    reasons
        .iter()
        .find(|reason| !REASONS.contains(&reason.as_str()))
        .map(|reason| {
            format!(
                "a row no sealed execution decided names {reason:?}, which is no reason a sealed \
                 run gives"
            )
        })
}

/// What sealed executions establish, decided again from what each came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settled<'a> {
    /// One detected the mutation, and this is the target whose execution did first.
    Killed(&'a str),
    /// Every one passed.
    Survived,
    /// None ran: nothing sealed reaches the mutation.
    Unreached,
    /// One established nothing, and none detected the mutation.
    Nothing,
}

impl Settled<'_> {
    /// What a reader is told it establishes.
    const fn said(self) -> &'static str {
        match self {
            Self::Killed(_) => KILLED,
            Self::Survived => SURVIVED,
            Self::Unreached => UNREACHED,
            Self::Nothing => "no verdict",
        }
    }
}

/// Why `outcome`, killed by `killed_by`, is not what `executions` establish, or nothing where it is.
fn sealed_against(
    executions: &[SealedRun],
    outcome: &str,
    killed_by: Option<&str>,
) -> Option<String> {
    let mut settled = if executions.is_empty() {
        Settled::Unreached
    } else {
        Settled::Survived
    };
    let mut measured = false;
    for SealedRun {
        target, came_to, ..
    } in executions
    {
        if DETECTIONS.contains(&came_to.as_str()) {
            settled = Settled::Killed(target);
            measured = true;
            break;
        }
        if came_to == SET_ASIDE {
            continue;
        }
        measured = true;
        if DOUBTS.contains(&came_to.as_str()) {
            settled = Settled::Nothing;
        } else if came_to != PASSED {
            return Some(format!(
                "a sealed execution of {target} came to {came_to:?}, which no sealed execution \
                 comes to"
            ));
        }
    }
    if !measured && !executions.is_empty() {
        settled = Settled::Nothing;
    }
    match (outcome, settled) {
        (KILLED, Settled::Killed(first)) => match killed_by {
            Some(by) if by == first => None,
            Some(by) => Some(format!(
                "the kill names {by} and its sealed executions detected it first in {first}"
            )),
            None => Some(format!(
                "the kill names no target and its sealed executions detected it first in {first}"
            )),
        },
        (SURVIVED | EQUIVALENT, Settled::Survived) | (UNREACHED, Settled::Unreached) => None,
        (_, settled) => Some(format!(
            "outcome {outcome} rests on sealed executions that establish {}",
            settled.said()
        )),
    }
}

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

/// The list `key` of the flat part `document`, which its schema requires.
///
/// # Errors
/// [`crate::route::ReadCauseError::Absent`] where it is not there or is not an array.
fn rows<'a>(
    document: &'a serde_json::Value,
    key: &str,
) -> Result<&'a [serde_json::Value], crate::route::ReadCauseError> {
    crate::route::required(document, key, serde_json::Value::as_array).map(Vec::as_slice)
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
