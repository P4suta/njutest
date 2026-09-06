// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving a session: judging every mutant it holds, and what a run shares between the ones it is measuring at once.

use std::sync::{PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::EngineError;
use crate::catalog::Mutant;
use crate::outcome::Outcome;
use crate::runner::Cancel;
use crate::session::{Request, Session};
use serde::{Deserialize, Serialize};

/// The machine: shared while a run measures several mutations at once, and given to one of them when a budget expires.
///
/// A mutation's budget is a multiple of a duration the baseline measured, and
/// a duration measured while three other test processes were running is a
/// fact about the load rather than about the mutation. A run that has to
/// decide whether a budget really expired takes the machine to itself first,
/// so the measurement the decision rests on is the one the budget was
/// calibrated for. It is not a retry policy: one expired budget buys one
/// quiet measurement, and what that measurement observes is what stands.
#[derive(Debug, Default)]
pub struct Quiet(RwLock<()>);

impl Quiet {
    /// Runs `work` beside whatever else this run is measuring.
    pub fn shared<R>(&self, work: impl FnOnce() -> R) -> R {
        let held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        let answer = work();
        drop(held);
        answer
    }

    /// Runs `work` with nothing else this run started running beside it.
    pub fn alone<R>(&self, work: impl FnOnce() -> R) -> R {
        let held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        let answer = work();
        drop(held);
        answer
    }
}

/// One mutant whose outcome a reviewer declared in advance, so the run verifies the claim instead of hiding the mutant.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    /// The mutant, by identity or by a prefix that names exactly one.
    pub id: String,
    /// Why the outcome is what it is. Required: an expectation without a reason is a suppression, and a report cannot audit one.
    pub reason: String,
    /// The outcome the run must confirm.
    #[serde(
        default = "expected_by_default",
        deserialize_with = "outcome",
        serialize_with = "outcome_name"
    )]
    pub outcome: Outcome,
}

const fn expected_by_default() -> Outcome {
    Outcome::Survived
}

/// An outcome, as a person writes it.
fn outcome<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Outcome, D::Error> {
    let text = String::deserialize(deserializer)?;
    Outcome::parse(&text).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "{text:?} is not an outcome; write {}",
            Outcome::ALL.map(Outcome::name).join(", ")
        ))
    })
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with hands the field by reference, whatever its shape"
)]
fn outcome_name<S: serde::Serializer>(value: &Outcome, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.name())
}

/// The exit code of a run that established detection for everything it executed.
pub const EXIT_DETECTED: u8 = 0;

/// The exit code of a run that left something the tests did not notice.
pub const EXIT_UNDETECTED: u8 = 1;

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = 130;

/// The exit code of a run that established nothing: it failed rather than answered.
pub const EXIT_FAILED: u8 = 2;

/// What one mutant's execution established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// The dense catalog index the guards name.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// What the execution says.
    pub outcome: Outcome,
    /// The target that ran, empty when none did.
    pub target: String,
    /// The exit status of the last execution.
    pub exit_code: i32,
    /// How long every execution of this mutant took together.
    pub duration: Duration,
    /// How many tests ran, when the harness said.
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active, which is what noticed it.
    pub failed_tests: Vec<String>,
    /// The signal the last execution died from, on the platforms that have them.
    pub signal: Option<i32>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// The run that established this, when it was not this one.
    pub source_run_id: Option<String>,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
    /// Whether the coverage measurement proved no target reaches it, which is why it never ran.
    pub unreached: bool,
    /// Why it was never executed, when it was not.
    pub not_run_reason: Option<NotRunReason>,
    /// Which targets could have noticed it, and which of them ran.
    pub route: Option<crate::report::run::RouteDocument>,
}

/// Whether a reviewer's claim about one mutant held.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Standing {
    /// The run confirmed it.
    Met,
    /// The run contradicted it.
    Stale {
        /// What the run established instead.
        actual: Outcome,
    },
    /// The claim names no mutant of this catalog, so nothing confirms or contradicts it.
    Unmatched {
        /// Why the identity resolved to nothing.
        why: String,
    },
}

/// One declared expectation, as the run left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The identity or prefix the file wrote.
    pub id: String,
    /// Why the reviewer claims the outcome.
    pub reason: String,
    /// The outcome claimed.
    pub outcome: Outcome,
    /// The mutant it resolved to, when it resolved.
    pub mutant: Option<String>,
    /// Whether the claim held.
    pub standing: Standing,
}

/// What kind of hole a finding names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FindingKind {
    /// Every test passed with the mutant active.
    SurvivingMutant,
    /// The run could not decide.
    InconclusiveMutant,
    /// The harness itself failed for this mutant.
    ErroredMutant,
    /// A mutant nothing ran and nothing cancelled.
    NotRunMutant,
    /// No measured target reaches the mutant, so no test could have noticed it.
    UnreachedMutant,
    /// A reviewer's claim the run contradicted.
    StaleExpectation,
    /// A reviewer's claim that names no mutant of this catalog.
    UnmatchedExpectation,
}

impl FindingKind {
    /// Every kind, in the order findings are reported.
    pub const ALL: [Self; 7] = [
        Self::SurvivingMutant,
        Self::InconclusiveMutant,
        Self::ErroredMutant,
        Self::NotRunMutant,
        Self::UnreachedMutant,
        Self::StaleExpectation,
        Self::UnmatchedExpectation,
    ];

    /// The canonical wire name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SurvivingMutant => "surviving-mutant",
            Self::InconclusiveMutant => "inconclusive-mutant",
            Self::ErroredMutant => "errored-mutant",
            Self::NotRunMutant => "not-run-mutant",
            Self::UnreachedMutant => "unreached-mutant",
            Self::StaleExpectation => "stale-expectation",
            Self::UnmatchedExpectation => "unmatched-expectation",
        }
    }

    /// The kind with the given wire name, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether the finding is about the run itself rather than about the tests.
    #[must_use]
    pub const fn is_infrastructure(self) -> bool {
        matches!(self, Self::ErroredMutant | Self::NotRunMutant)
    }
}

/// One thing that stops a run from being clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What kind of hole it is.
    pub kind: FindingKind,
    /// The mutant it is about, when it is about one.
    pub mutant: Option<String>,
    /// One sentence a reader can act on.
    pub detail: String,
}

/// What a run counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// How many mutants the compiler accepted.
    pub cataloged: u32,
    /// How many candidates the compiler refused.
    pub refused: u32,
    /// How many places discovery passed over.
    pub skipped: u32,
    /// How many mutants an execution reached a verdict on.
    pub executed: u32,
    /// How many a test failed on.
    pub killed: u32,
    /// How many every test passed on.
    pub survived: u32,
    /// How many exceeded the budget twice.
    pub timed_out: u32,
    /// How many the run could not decide.
    pub inconclusive: u32,
    /// How many the harness itself failed on.
    pub errored: u32,
    /// How many never ran.
    pub not_run: u32,
    /// How many of those never ran because no measured target reaches them.
    pub unreached: u32,
    /// How many survivors a reviewer had declared, and the run confirmed.
    pub expected: u32,
}

/// The share of decided mutants the tests noticed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Killed plus confirmed timeouts.
    pub detected: u32,
    /// Detected plus survived: the mutants the run has an answer for.
    pub decided: u32,
    /// `detected / decided`, between zero and one.
    pub value: f64,
}

/// Everything one run established.
#[derive(Debug, Clone)]
pub struct Run {
    /// One record per cataloged mutant, in catalog order.
    pub judged: Vec<Judged>,
    /// The declared expectations, as the run left them.
    pub expectations: Vec<Verified>,
    /// How many places discovery passed over.
    pub skipped: u32,
    /// How many candidates the compiler refused.
    pub refused: u32,
    /// Whether the run stopped because it was asked to.
    pub interrupted: bool,
    /// Which part of the catalog this run was about, when it was about one.
    pub shard: Option<Shard>,
    /// How long the executions took together.
    pub duration: Duration,
}

impl Run {
    /// The outcomes folded. An outcome this release does not know counts with the harness failures: nothing it says can be read as detection.
    #[must_use]
    pub fn tally(&self) -> Tally {
        let mut tally = Tally {
            cataloged: count(self.judged.len()),
            refused: self.refused,
            skipped: self.skipped,
            ..Tally::default()
        };
        for one in &self.judged {
            let slot = match one.outcome {
                Outcome::Killed => &mut tally.killed,
                Outcome::Survived => &mut tally.survived,
                Outcome::TimedOut => &mut tally.timed_out,
                Outcome::Inconclusive => &mut tally.inconclusive,
                Outcome::NotRun => &mut tally.not_run,
                _ => &mut tally.errored,
            };
            *slot = slot.saturating_add(1);
            if one.unreached {
                tally.unreached = tally.unreached.saturating_add(1);
            }
            if one.expected {
                tally.expected = tally.expected.saturating_add(1);
            }
        }
        tally.executed = tally.cataloged.saturating_sub(tally.not_run);
        tally
    }

    /// The share of decided mutants the tests noticed, or `None` when the run decided nothing.
    #[must_use]
    pub fn score(&self) -> Option<Score> {
        let tally = self.tally();
        let detected = tally.killed.saturating_add(tally.timed_out);
        let decided = detected.saturating_add(tally.survived);
        (decided > 0).then(|| Score {
            detected,
            decided,
            value: f64::from(detected) / f64::from(decided),
        })
    }

    /// Everything that stops the run from being clean, mutants first and in catalog order. An outcome this release does not know counts as a harness failure: nothing it says can be read as detection.
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        for one in &self.judged {
            let kind = match one.outcome {
                Outcome::Survived if one.expected => continue,
                Outcome::Survived => FindingKind::SurvivingMutant,
                Outcome::Inconclusive => FindingKind::InconclusiveMutant,
                Outcome::NotRun if one.unreached => FindingKind::UnreachedMutant,
                Outcome::NotRun if self.interrupted => continue,
                Outcome::NotRun => FindingKind::NotRunMutant,
                Outcome::Killed | Outcome::TimedOut => continue,
                _ => FindingKind::ErroredMutant,
            };
            findings.push(Finding {
                kind,
                mutant: Some(one.id.clone()),
                detail: detail(kind, one),
            });
        }
        for expectation in &self.expectations {
            match &expectation.standing {
                Standing::Met => {}
                Standing::Stale { actual } => findings.push(Finding {
                    kind: FindingKind::StaleExpectation,
                    mutant: expectation.mutant.clone(),
                    detail: format!(
                        "{:?} was expected to be {}, and the run says {}; the claim {:?} no longer holds",
                        expectation.id,
                        expectation.outcome.name(),
                        actual.name(),
                        expectation.reason
                    ),
                }),
                Standing::Unmatched { why } => findings.push(Finding {
                    kind: FindingKind::UnmatchedExpectation,
                    mutant: None,
                    detail: format!("the expectation for {:?} verifies nothing: {why}", expectation.id),
                }),
            }
        }
        findings
    }

    /// The exit code the run earns.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if self.interrupted {
            return EXIT_INTERRUPTED;
        }
        let findings = self.findings();
        if findings
            .iter()
            .any(|finding| finding.kind.is_infrastructure())
        {
            return EXIT_FAILED;
        }
        if findings.is_empty() {
            EXIT_DETECTED
        } else {
            EXIT_UNDETECTED
        }
    }
}

fn detail(kind: FindingKind, one: &Judged) -> String {
    match kind {
        FindingKind::SurvivingMutant => format!(
            "no test noticed {}; {} ran and passed",
            one.display_id,
            one.tests_run
                .map_or_else(|| "the target".to_owned(), |count| format!("{count} tests"))
        ),
        FindingKind::InconclusiveMutant if one.retried => format!(
            "{} timed out once and did not do so again, so the run cannot say what the tests \
             noticed",
            one.display_id
        ),
        FindingKind::InconclusiveMutant => format!(
            "no test ran with {} active on {}, so the run cannot say what the tests noticed",
            one.display_id,
            if one.target.is_empty() {
                "any target"
            } else {
                &one.target
            }
        ),
        FindingKind::ErroredMutant => format!(
            "the harness itself failed on {} with exit {}, so nothing about the tests was \
             established",
            one.display_id, one.exit_code
        ),
        FindingKind::UnreachedMutant => format!(
            "no measured test reaches {}: the mutation lives in code the tests never execute",
            one.display_id
        ),
        FindingKind::NotRunMutant
        | FindingKind::StaleExpectation
        | FindingKind::UnmatchedExpectation => format!(
            "{} was never executed and nothing cancelled the run",
            one.display_id
        ),
    }
}

/// One length as a count. A catalog larger than a `u32` is one no run could hold in memory to begin with.
#[must_use]
pub fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// What a run needs beyond the session itself.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// How long one execution may take before it is retried serially.
    /// The claims to verify.
    pub expectations: &'a [Expectation],
    /// The machine, which a confirming retry takes to itself.
    pub quiet: &'a Quiet,
    /// Further arguments for the harness.
    pub args: &'a [String],
    /// Which part of the catalog this run is about. `None` is all of it.
    pub shard: Option<Shard>,
    /// Where what earlier runs of this exact tree established is kept, and this run's own name. `None` establishes everything afresh.
    pub outcomes: Option<Reusing<'a>>,
}

/// Where a run reads and writes what is established about individual mutants.
#[derive(Debug, Clone, Copy)]
pub struct Reusing<'a> {
    /// The records.
    pub store: &'a crate::outcomes::Store,
    /// Everything a key is computed from beyond the mutant's own identity.
    pub keyed: &'a crate::outcomes::Keyed,
    /// This run, which is what a record it writes names.
    pub run_id: &'a str,
}

/// One part of a catalog, for a run that shares the work with others.
///
/// The parts are cut by catalog index, which is dense and in the catalog's own
/// order, so every run of the same tree cuts them the same way without any run
/// having to know what the others chose. They balance by count rather than by
/// cost: a shard holding the slow mutants is a shard that takes longer, and
/// that is a thing to measure before it is a thing to solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    /// Which part, from one.
    pub index: u32,
    /// How many parts there are.
    pub of: u32,
}

/// Why a shard is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShardError {
    /// The text is not `K/N`.
    #[error("{text:?} is not a shard; write it as K/N, as in 1/4")]
    Malformed {
        /// The text as given.
        text: String,
    },
    /// A count of zero parts, or a part outside them.
    #[error("shard {index} of {of} is not a part of anything; K runs from 1 to N")]
    OutOfRange {
        /// The part asked for.
        index: u32,
        /// How many were said to exist.
        of: u32,
    },
}

impl Shard {
    /// The shard `K/N` names.
    ///
    /// # Errors
    /// See [`ShardError`].
    pub fn parse(text: &str) -> Result<Self, ShardError> {
        let malformed = || ShardError::Malformed {
            text: text.to_owned(),
        };
        let (index, of) = text.split_once('/').ok_or_else(malformed)?;
        let index: u32 = index.trim().parse().map_err(|_error| malformed())?;
        let of: u32 = of.trim().parse().map_err(|_error| malformed())?;
        if of == 0 || index == 0 || index > of {
            return Err(ShardError::OutOfRange { index, of });
        }
        Ok(Self { index, of })
    }

    /// Whether the mutant at this catalog index belongs to this part.
    #[must_use]
    pub const fn holds(self, index: u32) -> bool {
        match index.checked_rem(self.of) {
            Some(part) => part == self.index.saturating_sub(1),
            None => false,
        }
    }
}

impl std::fmt::Display for Shard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.index, self.of)
    }
}

/// Runs every accepted mutant of `session` once, retrying a timeout serially before believing it, and reports what the run established.
///
/// `progress` is told about each mutant as it finishes, so a command line can say where it is without this module knowing what a stream is.
///
/// # Errors
/// Returns what the engine could not do. A mutant the engine refuses to execute is recorded as errored rather than ending the run.
pub fn run<O: Observer>(
    session: &Session,
    options: &Options<'_>,
    cancel: &Cancel,
    observer: &mut O,
) -> Result<Run, EngineError> {
    let started = Instant::now();
    let accepted: Vec<u32> = session
        .accepted()
        .iter()
        .copied()
        .filter(|index| options.shard.is_none_or(|shard| shard.holds(*index)))
        .collect();
    let total = count(accepted.len());
    let mut judged = Vec::with_capacity(accepted.len());
    let mut interrupted = false;
    for (position, index) in accepted.iter().enumerate() {
        let Some(mutant) = session.catalog().by_index(*index) else {
            continue;
        };
        if cancel.is_cancelled() {
            interrupted = true;
            judged.push(unexecuted(mutant, NotRunReason::Interrupted));
            continue;
        }
        observer.started(mutant);
        let one = if let Some(one) = reuse(mutant, options) {
            one
        } else {
            let established = execute(session, mutant, options, cancel)?;
            keep(mutant, options, &established);
            established
        };
        let mut one = one;
        route(session, mutant, &mut one);
        observer.judged(&one, count(position).saturating_add(1), total);
        judged.push(one);
    }
    Ok(Run {
        judged,
        expectations: Vec::new(),
        skipped: session
            .skips()
            .iter()
            .fold(0u32, |total, skip| total.saturating_add(skip.count)),
        refused: count(session.rejections().len()),
        interrupted: interrupted || cancel.is_cancelled(),
        shard: options.shard,
        duration: started.elapsed(),
    })
}

/// Records which targets could have noticed this mutation and which of them ran.
///
/// One record per judged mutant, whatever became of it: a mutant nothing
/// reached leaves a route and no execution, and one an earlier run answered
/// for names that run rather than a target. The record is the only place a
/// reader can see a proof layer remove work, so it is written even when the
/// mutant was never started.
fn route(session: &Session, mutant: &Mutant, judged: &mut Judged) {
    let decided = session.route(mutant);
    let ran = if judged.source_run_id.is_some() || judged.outcome == Outcome::NotRun {
        Vec::new()
    } else {
        decided.executed(&judged.target, judged.outcome.detected())
    };
    judged.route = Some(crate::report::run::route_document(&decided, ran.clone()));
    if !session.trace().is_enabled() {
        return;
    }
    let mut record = decided.record(mutant, ran);
    record.reused.clone_from(&judged.source_run_id);
    session.trace().route(record);
}

/// What a caller hears while a run happens.
///
/// Every method is called on the thread that called [`run`], so an
/// implementation needs no synchronisation of its own and may borrow whatever
/// it likes. Each has a default that does nothing, so an observer implements
/// only what it draws.
pub trait Observer {
    /// A mutant is about to be judged.
    fn started(&mut self, _mutant: &Mutant) {}

    /// A mutant has been judged. `completed` counts what has been delivered, of `total`.
    fn judged(&mut self, _judged: &Judged, _completed: u32, _total: u32) {}
}

/// An observer that draws nothing, for a caller that reads the report instead.
#[derive(Debug, Default, Clone, Copy)]
pub struct Silent;

impl Observer for Silent {}

/// Why a mutant was never executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotRunReason {
    /// The coverage measurement proved no target reaches it.
    Unreached,
    /// A proof discharged every target it could have been noticed by.
    Discharged,
    /// The run was interrupted before it got there.
    Interrupted,
}

impl NotRunReason {
    /// The kebab-case name a report and a recording use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::Discharged => "discharged",
            Self::Interrupted => "interrupted",
        }
    }
}

/// One mutant, executed once and — when it timed out — once more on its own before the timeout is believed.
fn execute(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    cancel: &Cancel,
) -> Result<Judged, EngineError> {
    let request = Request::new(mutant.id.clone()).with_args(options.args.to_vec());
    let judgement = session.judge(&request, options.quiet, cancel)?;
    let duration = judgement.duration();
    let result = judgement.result;
    Ok(Judged {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        outcome: result.outcome,
        target: result.target,
        exit_code: result.exit_code,
        duration,
        tests_run: result.tests_run,
        failed_tests: result.failed_tests,
        signal: result.signal,
        retried: judgement.retried,
        expected: false,
        unreached: session.reaches(mutant) == Some(false),
        not_run_reason: None,
        route: None,
        source_run_id: None,
    })
}

/// What an earlier run of this exact tree established about this mutant, when a record answers for it.
fn reuse(mutant: &Mutant, options: &Options<'_>) -> Option<Judged> {
    let reusing = options.outcomes?;
    let (outcome, record) = reusing
        .store
        .get(&reusing.keyed.key(&mutant.id), &mutant.id)?;
    Some(Judged {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        outcome,
        target: record.target,
        exit_code: 0,
        duration: Duration::ZERO,
        tests_run: record.tests_run,
        failed_tests: record.failed_tests,
        signal: None,
        retried: false,
        expected: false,
        unreached: false,
        not_run_reason: None,
        route: None,
        source_run_id: Some(record.run_id),
    })
}

/// Records what this run established, for the next run of this exact tree. Only an outcome about the mutant is kept: a run that could not decide, or that never ran, says nothing the next run could inherit.
fn keep(mutant: &Mutant, options: &Options<'_>, judged: &Judged) {
    let Some(reusing) = options.outcomes else {
        return;
    };
    if !matches!(
        judged.outcome,
        Outcome::Killed | Outcome::Survived | Outcome::TimedOut
    ) {
        return;
    }
    reusing.store.put(
        &reusing.keyed.key(&mutant.id),
        &crate::outcomes::Record {
            schema: crate::outcomes::SCHEMA.to_owned(),
            mutant: mutant.id.clone(),
            outcome: judged.outcome.name().to_owned(),
            target: judged.target.clone(),
            tests_run: judged.tests_run,
            failed_tests: judged.failed_tests.clone(),
            run_id: reusing.run_id.to_owned(),
        },
    );
}

fn unexecuted(mutant: &Mutant, reason: NotRunReason) -> Judged {
    Judged {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        outcome: Outcome::NotRun,
        target: String::new(),
        exit_code: crate::runner::EXIT_CODE_UNAVAILABLE,
        duration: Duration::ZERO,
        tests_run: None,
        failed_tests: Vec::new(),
        signal: None,
        retried: false,
        expected: false,
        unreached: false,
        not_run_reason: Some(reason),
        route: None,
        source_run_id: None,
    }
}

/// Resolves every declared expectation against what the run established, and marks the mutants a reviewer accounted for.
#[must_use]
pub fn verify(
    session: &Session,
    expectations: &[Expectation],
    judged: &mut [Judged],
) -> Vec<Verified> {
    expectations
        .iter()
        .map(|expectation| {
            let resolved = session.resolve(&expectation.id);
            let (mutant, standing) = match resolved {
                Err(error) => (
                    None,
                    Standing::Unmatched {
                        why: error.to_string(),
                    },
                ),
                Ok(mutant) => {
                    let id = mutant.id.clone();
                    let found = judged.iter_mut().find(|one| one.id == id);
                    match found {
                        None => (
                            Some(id),
                            Standing::Unmatched {
                                why: "the mutant is in the catalog and the run did not reach it"
                                    .to_owned(),
                            },
                        ),
                        Some(one) if one.outcome == expectation.outcome => {
                            one.expected = true;
                            (Some(id), Standing::Met)
                        }
                        Some(one) => (
                            Some(id),
                            Standing::Stale {
                                actual: one.outcome,
                            },
                        ),
                    }
                }
            };
            Verified {
                id: expectation.id.clone(),
                reason: expectation.reason.clone(),
                outcome: expectation.outcome,
                mutant,
                standing,
            }
        })
        .collect()
}
