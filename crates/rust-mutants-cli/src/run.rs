// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running every mutant of a session once, and what the run adds up to.

use std::time::{Duration, Instant};

use rust_mutants::EngineError;
use rust_mutants::catalog::Mutant;
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Request, Session};

use crate::config::Expectation;

/// The exit code of a run that established detection for everything it executed.
pub const EXIT_DETECTED: u8 = 0;

/// The exit code of a run that left something the tests did not notice.
pub const EXIT_UNDETECTED: u8 = 1;

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = 130;

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
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// The run that established this, when it was not this one.
    pub source_run_id: Option<String>,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
    /// Whether the coverage measurement proved no target reaches it, which is why it never ran.
    pub unreached: bool,
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
            return crate::EXIT_USAGE;
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
        FindingKind::InconclusiveMutant => format!(
            "{} timed out once and did not do so again, so the run cannot say what the tests \
             noticed",
            one.display_id
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
    pub timeout: Duration,
    /// The claims to verify.
    pub expectations: &'a [Expectation],
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
pub fn run(
    session: &Session,
    options: &Options<'_>,
    cancel: &Cancel,
    progress: &mut dyn FnMut(&Judged, u32, u32),
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
            judged.push(unexecuted(mutant));
            continue;
        }
        let one = if let Some(one) = reuse(mutant, options) {
            one
        } else {
            let established = execute(session, mutant, options, cancel)?;
            keep(mutant, options, &established);
            established
        };
        progress(&one, count(position).saturating_add(1), total);
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

/// One mutant, executed once and — when it timed out — once more on its own before the timeout is believed.
fn execute(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    cancel: &Cancel,
) -> Result<Judged, EngineError> {
    let request = Request {
        mutant: mutant.id.clone(),
        target: None,
        test: None,
        args: options.args.to_vec(),
        timeout: Some(options.timeout),
    };
    let first = session.exec(&request, cancel)?;
    let mut duration = first.duration;
    let mut retried = false;
    let mut result = first;
    if result.outcome == Outcome::TimedOut && !cancel.is_cancelled() {
        retried = true;
        let again = session.exec(&request, cancel)?;
        duration = duration.saturating_add(again.duration);
        result = again;
        if result.outcome != Outcome::TimedOut && !result.outcome.detected() {
            result.outcome = Outcome::Inconclusive;
        }
    }
    Ok(Judged {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        outcome: result.outcome,
        target: result.target,
        exit_code: result.exit_code,
        duration,
        tests_run: result.tests_run,
        retried,
        expected: false,
        unreached: session.reaches(mutant) == Some(false),
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
        retried: false,
        expected: false,
        unreached: false,
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
            run_id: reusing.run_id.to_owned(),
        },
    );
}

fn unexecuted(mutant: &Mutant) -> Judged {
    Judged {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        outcome: Outcome::NotRun,
        target: String::new(),
        exit_code: rust_mutants::runner::EXIT_CODE_UNAVAILABLE,
        duration: Duration::ZERO,
        tests_run: None,
        retried: false,
        expected: false,
        unreached: false,
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
