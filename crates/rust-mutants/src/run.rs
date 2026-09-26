// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving a session: judging every mutant it holds, and what a run shares between the ones it is measuring at once.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::EngineError;
use crate::catalog::Mutant;
use crate::discover::SkipClaim;
use crate::outcome::Outcome;
use crate::runner::Cancel;
use crate::session::{LocateError, Locator, Request, Session};
use crate::workspace::SessionError;

/// The machine: shared while a run measures several mutations at once, and given to one of them when a budget expires.
#[derive(Debug, Default)]
pub struct Quiet(RwLock<()>);

impl Quiet {
    /// Runs `work` beside whatever else this run is measuring.
    ///
    /// # Errors
    /// Returns a typed session failure after any panic poisons the coordination lock; continuing could otherwise run a supposedly isolated retry beside work whose state is unknown.
    pub fn shared<R>(&self, work: impl FnOnce() -> R) -> Result<R, EngineError> {
        let held = self
            .0
            .read()
            .map_err(|_poisoned| SessionError::CoordinationPoisoned)?;
        let answer = work();
        drop(held);
        Ok(answer)
    }

    /// Runs `work` with nothing else this run started running beside it.
    ///
    /// # Errors
    /// Returns a typed session failure when prior work poisoned the lock.
    pub fn alone<R>(&self, work: impl FnOnce() -> R) -> Result<R, EngineError> {
        let held = self
            .0
            .write()
            .map_err(|_poisoned| SessionError::CoordinationPoisoned)?;
        let answer = work();
        drop(held);
        Ok(answer)
    }
}

/// One mutant a reviewer declared, with the outcome the run must confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// The mutant by identity or by a prefix that names exactly one.
    pub id: Option<String>,
    /// The mutant by where it is and what it edits.
    pub locator: Option<Locator>,
    /// Why the outcome is what it is.
    /// Required: an expectation without a reason is a suppression, and a report cannot audit one.
    pub reason: String,
    /// The outcome the run must confirm.
    pub outcome: Outcome,
    /// Where the claim is judged, which is everywhere unless it names facts (ADR 0042).
    pub under: Where,
}

/// The facts a claim was established under: a `cfg` over the target and exact values of the tests' environment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Where {
    /// The predicate over the target that must hold, when the claim names one.
    pub cfg: Option<crate::facts::Predicate>,
    /// Each name of the environment the tests are given, with the value it must have.
    pub env: std::collections::BTreeMap<String, String>,
}

/// Why a claim was not judged in this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheld {
    /// The file it names is one no unit of this build read.
    NotCompiled,
    /// The target does not satisfy the `cfg` it names.
    Cfg {
        /// The predicate, as the claim wrote it.
        predicate: String,
    },
    /// The environment the tests are given does not hold a value the claim names.
    Env {
        /// The name.
        name: String,
        /// Whether the tests are given the name at all; its value is never written anywhere, since it may be a secret.
        given: bool,
    },
}

impl Unheld {
    /// The fact that did not hold, as a reader reads it.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::NotCompiled => "no unit of this build compiled the file it names".to_owned(),
            Self::Cfg { predicate } => format!("the target does not satisfy cfg({predicate})"),
            Self::Env { name, given: true } => format!(
                "the tests are given {name} with another value than the one the claim holds under"
            ),
            Self::Env { name, given: false } => format!("the tests are given no {name}"),
        }
    }
}

impl Expectation {
    /// How the claim is written back to a reader.
    #[must_use]
    pub fn name(&self) -> String {
        match (&self.id, &self.locator) {
            (Some(id), _) => id.clone(),
            (None, Some(locator)) => locator.name(),
            (None, None) => String::new(),
        }
    }
}

/// The exit code of a run that established detection for everything it executed.
pub const EXIT_DETECTED: u8 = Exit::Detected.code();

/// The exit code of a run that reported a finding about the tests.
pub const EXIT_FOUND: u8 = Exit::Found.code();

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = Exit::Interrupted.code();

/// The exit code of a run that established nothing: it failed rather than answered.
pub const EXIT_FAILED: u8 = Exit::Unestablished.code();

/// What the optional compiler-artifact comparison established.
///
/// `NotMeasured` and `NotEstablished` are different: the latter says the layer ran but its premises did not support either identity or difference.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum CodegenIdentity {
    /// The comparison layer was not run for this mutation.
    NotMeasured,
    /// Every retained executable was byte-identical.
    Identical,
    /// At least one retained executable differed.
    Different,
    /// The comparison ran but its premises established neither answer.
    NotEstablished,
}

/// What one mutant's execution established.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is an independent fact about one judged mutant that the published report states"
)]
pub struct Judged {
    /// The dense catalog index the guards name.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// What the execution says.
    pub outcome: Outcome,
    /// The verified execution notice when the step limit was reached.
    pub step_notice: Option<crate::execute::StepLimitNotice>,
    /// The target that ran, empty when none did.
    pub target: String,
    /// The exit status of the last execution.
    pub exit_code: i32,
    /// Why the last execution's process never started, where it did not.
    pub start_failure: Option<crate::execute::StartFailure>,
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
    /// Whether the harness had already answered when the clock ended the process.
    pub lingered: bool,
    /// The run that established this, when it was not this one.
    pub source_run_id: Option<String>,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
    /// Why it was never executed, when it was not.
    pub not_run_reason: Option<NotRunReason>,
    /// Which targets could have noticed it, and which of them ran.
    pub route: Option<crate::report::run::RouteDocument>,
    /// Whether this run measured it, rather than reusing what an earlier one established or never reaching it.
    /// A measurement records its own route.
    pub measured: bool,
    /// What comparison of the compiler artifacts established.
    pub identical: CodegenIdentity,
    /// Each test that declined to measure in the execution the outcome rests on, in its words (ADR 0043).
    pub declined: Vec<crate::decline::Decline>,
}

/// Whether a reviewer's claim about one mutant held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The run confirmed it.
    Met,
    /// The run confirmed it, and the code it is about has moved since it was written.
    Moved {
        /// The line the claim named.
        from: u32,
        /// The line the mutation is on now.
        to: u32,
    },
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
    /// The claim names mutations of this catalog, and this run decided none of them: a selection left them out, another shard holds them, or the run stopped first.
    Unjudged,
    /// The claim is not judged here, because a fact it was established under does not hold (ADR 0042).
    Inapplicable {
        /// The fact.
        because: Unheld,
    },
}

/// One declared expectation, as the run left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The identity, prefix, or locator the file wrote, as a reader reads it.
    pub id: String,
    /// The locator the file wrote, when it wrote one.
    pub locator: Option<Locator>,
    /// Why the reviewer claims the outcome.
    pub reason: String,
    /// The outcome claimed.
    pub outcome: Outcome,
    /// The mutant it resolved to, when it resolved.
    /// The one that decided the standing, when it named several.
    pub mutant: Option<String>,
    /// How many mutants the claim was resolved against, which is one unless the locator stated a count.
    pub covered: u32,
    /// Whether the claim held.
    pub standing: Standing,
    /// Where the claim is judged, as the file wrote it.
    pub under: Where,
}

/// What kind of hole a finding names.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// Every test passed with the mutant active.
    SurvivingMutant,
    /// The run could not decide.
    InconclusiveMutant,
    /// A process reached the selected guard's execution allowance, which does not decide why.
    StepLimitReachedMutant,
    /// This machine stopped waiting for the mutant, so the run established nothing about it.
    WaitedMutant,
    /// The harness itself failed for this mutant.
    ErroredMutant,
    /// A mutant nothing ran and nothing cancelled.
    NotRunMutant,
    /// No measured target reaches the mutant, so no test could have noticed it.
    UnreachedMutant,
    /// A proof removed every target that could have noticed the mutant, so no test could have.
    DischargedMutant,
    /// A reviewer's claim the run contradicted.
    StaleExpectation,
    /// A reviewer's claim that names no mutant of this catalog.
    UnmatchedExpectation,
    /// A `rust-mutants: skip` marker that hid nothing, which is a claim about code that has moved or gone.
    UnmatchedSkip,
}

impl FindingKind {
    /// The canonical wire name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SurvivingMutant => "surviving-mutant",
            Self::InconclusiveMutant => "inconclusive-mutant",
            Self::StepLimitReachedMutant => "step-limit-reached-mutant",
            Self::WaitedMutant => "waited-mutant",
            Self::ErroredMutant => "errored-mutant",
            Self::NotRunMutant => "not-run-mutant",
            Self::UnreachedMutant => "unreached-mutant",
            Self::DischargedMutant => "discharged-mutant",
            Self::StaleExpectation => "stale-expectation",
            Self::UnmatchedExpectation => "unmatched-expectation",
            Self::UnmatchedSkip => "unmatched-skip",
        }
    }

    /// The canonical wire name, for interfaces that take a string slice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.name()
    }

    /// The kind with the given wire name, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// How a run holding this finding ends: a finding about the tests, or one about what the run could not measure.
    #[must_use]
    pub const fn exit(self) -> Exit {
        match self {
            Self::SurvivingMutant
            | Self::InconclusiveMutant
            | Self::UnreachedMutant
            | Self::DischargedMutant
            | Self::StaleExpectation
            | Self::UnmatchedExpectation
            | Self::UnmatchedSkip => Exit::Found,
            Self::StepLimitReachedMutant
            | Self::WaitedMutant
            | Self::ErroredMutant
            | Self::NotRunMutant => Exit::Unestablished,
        }
    }

    /// Whether the finding is about the run itself rather than about the tests.
    #[must_use]
    pub const fn is_infrastructure(self) -> bool {
        matches!(self.exit(), Exit::Unestablished)
    }
}

/// How a run ends, which is the one table its exit code, its `--help` and its documentation are read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Exit {
    /// Every mutant the run decided, the tests noticed.
    Detected,
    /// There is a finding about the tests.
    Found,
    /// The run could not measure something it ran, or failed, or was used wrongly.
    Unestablished,
    /// The run was interrupted.
    Interrupted,
    /// The run was terminated.
    Terminated,
}

impl Exit {
    /// The exit a process that ended with `code` meant, or nothing where the table holds no such code, which is a failure rather than any verdict.
    #[must_use]
    pub fn read(code: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|exit| i32::from(exit.code()) == code)
    }

    /// The code the process ends with.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Detected => 0,
            Self::Found => 1,
            Self::Unestablished => 2,
            Self::Interrupted => 130,
            Self::Terminated => 143,
        }
    }

    /// What it means, as `--help` and the documentation say it.
    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Detected => "every mutant the run decided, the tests noticed",
            Self::Found => {
                "there is a finding about the tests: a survivor, a mutation no test reached or a \
                 proof removed, a mutation the run could not decide either way, or a stale or \
                 unmatched claim"
            }
            Self::Unestablished => {
                "the run could not measure a mutation it ran — it waited, reached its step limit, \
                 errored or was not run — or the run itself failed, or the command was used wrongly"
            }
            Self::Interrupted => "it was interrupted",
            Self::Terminated => "it was terminated, which is what a cancelled job sends",
        }
    }

    /// How a run whose findings are of `kinds` ends, `interrupted` or not: the one decision every report of a run makes.
    #[must_use]
    pub fn of(interrupted: bool, kinds: impl IntoIterator<Item = FindingKind>) -> Self {
        if interrupted {
            return Self::Interrupted;
        }
        kinds
            .into_iter()
            .map(FindingKind::exit)
            .max()
            .unwrap_or(Self::Detected)
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
    /// How many candidate rows the run accounts for, excluding compiler refusals.
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
    /// How many reached the per-process guard-take limit without deciding the mutation.
    pub step_limit_reached: u32,
    /// How many this machine stopped waiting for, twice over.
    /// Not caught: the run established that it stopped waiting.
    pub waited: u32,
    /// How many the run could not decide.
    pub inconclusive: u32,
    /// How many the harness itself failed on.
    pub errored: u32,
    /// How many never ran.
    pub not_run: u32,
    /// How many of those never ran because no measured target reaches them.
    pub unreached: u32,
    /// How many of those never ran because a proof removed every target that could have noticed them.
    pub discharged: u32,
    /// How many of those measured nothing because every test that reached them declined to measure on this machine (ADR 0043).
    pub declined: u32,
    /// How many survivors a reviewer had declared, and the run confirmed.
    pub expected: u32,
}

/// The share of decided mutants the tests noticed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Mutants a test killed.
    pub detected: u32,
    /// Detected plus survived: the mutants the run has an answer for.
    pub decided: u32,
    /// `detected / decided`, between zero and one.
    pub value: f64,
}

/// Everything one run established.
#[derive(Debug, Clone)]
pub struct Run {
    /// One record per non-refused candidate the run accounts for, in catalog order.
    pub judged: Vec<Judged>,
    /// The declared expectations, as the run left them.
    pub expectations: Vec<Verified>,
    /// How many places discovery passed over.
    pub skipped: u32,
    /// How many candidates the compiler refused.
    pub refused: u32,
    /// Every `rust-mutants: skip` marker of a file the run measures.
    pub claims: Vec<SkipClaim>,
    /// Whether the run stopped because it was asked to.
    pub interrupted: bool,
    /// Which part of the catalog this run was about, when it was about one.
    pub shard: Option<Shard>,
    /// How long the executions took together.
    pub duration: Duration,
    /// How wide the run measured: what was asked for, and how many at once that came to on this machine.
    pub width: Width,
}

/// How wide a run measured, resolved once so the run and what it reports cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Width {
    /// What the configuration or the command line asked for.
    pub asked: Jobs,
    /// How many mutants were measured at once.
    pub used: usize,
}

impl Run {
    /// The outcomes folded.
    /// An outcome this release does not know counts with the harness failures: nothing it says can be read as detection.
    ///
    /// # Errors
    /// Refuses when the number of rows or any exact folded counter exceeds the durable `u32` report representation.
    pub fn tally(&self) -> Result<Tally, SessionError> {
        let mut tally = Tally {
            cataloged: count(self.judged.len())?,
            refused: self.refused,
            skipped: self.skipped,
            ..Tally::default()
        };
        for one in &self.judged {
            let slot = match one.outcome {
                Outcome::Killed => &mut tally.killed,
                Outcome::Survived => &mut tally.survived,
                Outcome::StepLimitReached => &mut tally.step_limit_reached,
                Outcome::Waited => &mut tally.waited,
                Outcome::Inconclusive => &mut tally.inconclusive,
                Outcome::NotRun => &mut tally.not_run,
                Outcome::Errored => &mut tally.errored,
            };
            *slot = slot.checked_add(1).ok_or(SessionError::RunCountOverflow)?;
            if one.not_run_reason == Some(NotRunReason::Unreached) {
                tally.unreached = tally
                    .unreached
                    .checked_add(1)
                    .ok_or(SessionError::RunCountOverflow)?;
            }
            if one.not_run_reason == Some(NotRunReason::Discharged) {
                tally.discharged = tally
                    .discharged
                    .checked_add(1)
                    .ok_or(SessionError::RunCountOverflow)?;
            }
            if one.not_run_reason == Some(NotRunReason::Declined) {
                tally.declined = tally
                    .declined
                    .checked_add(1)
                    .ok_or(SessionError::RunCountOverflow)?;
            }
            if one.expected {
                tally.expected = tally
                    .expected
                    .checked_add(1)
                    .ok_or(SessionError::RunCountOverflow)?;
            }
        }
        tally.executed = tally
            .cataloged
            .checked_sub(tally.not_run)
            .ok_or(SessionError::RunCountOverflow)?;
        Ok(tally)
    }

    /// The share of decided mutants the tests noticed, or `None` when the run decided nothing.
    ///
    /// # Errors
    /// Refuses when the run's exact accounting does not fit its durable counters.
    pub fn score(&self) -> Result<Option<Score>, SessionError> {
        let tally = self.tally()?;
        let detected = tally.killed;
        let decided = detected
            .checked_add(tally.survived)
            .ok_or(SessionError::RunCountOverflow)?;
        Ok((decided > 0).then(|| Score {
            detected,
            decided,
            value: f64::from(detected) / f64::from(decided),
        }))
    }

    /// Everything that stops the run from being clean, mutants first and in catalog order.
    /// An outcome this release does not know counts as a harness failure: nothing it says can be read as detection.
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        for one in &self.judged {
            let Some(kind) = verdict_finding(
                RowVerdict {
                    outcome: one.outcome,
                    not_run_reason: one.not_run_reason,
                    expected: one.expected,
                },
                self.interrupted,
            ) else {
                continue;
            };
            findings.push(Finding {
                kind,
                mutant: Some(one.id.clone()),
                detail: detail(kind, one),
            });
        }
        for expectation in &self.expectations {
            match &expectation.standing {
                Standing::Met
                | Standing::Moved { .. }
                | Standing::Unjudged
                | Standing::Inapplicable { .. } => {}
                Standing::Stale { actual } => findings.push(Finding {
                    kind: FindingKind::StaleExpectation,
                    mutant: expectation.mutant.clone(),
                    detail: stale_detail(
                        &expectation.id,
                        expectation.outcome,
                        *actual,
                        &expectation.reason,
                    ),
                }),
                Standing::Unmatched { why } => findings.push(Finding {
                    kind: FindingKind::UnmatchedExpectation,
                    mutant: None,
                    detail: unmatched_detail(&expectation.id, why),
                }),
            }
        }
        for claim in self.claims.iter().filter(|claim| !claim.matched) {
            findings.push(Finding {
                kind: FindingKind::UnmatchedSkip,
                mutant: None,
                detail: format!(
                    "the marker at {}:{} hides nothing: {:?}",
                    claim.path, claim.line, claim.reason
                ),
            });
        }
        findings
    }

    /// The exit code the run earns.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        Exit::of(
            self.interrupted,
            self.findings().iter().map(|finding| finding.kind),
        )
        .code()
    }
}

/// What a stale-expectation finding says about the claim it restates.
pub(crate) fn stale_detail(id: &str, claimed: Outcome, actual: Outcome, reason: &str) -> String {
    format!(
        "{id:?} was expected to be {}, and the run says {}; the claim {reason:?} no longer holds",
        claimed.name(),
        actual.name()
    )
}

/// What an unmatched-expectation finding says about the claim it restates.
pub(crate) fn unmatched_detail(id: &str, why: &str) -> String {
    format!("the expectation for {id:?} verifies nothing: {why}")
}

fn detail(kind: FindingKind, one: &Judged) -> String {
    match kind {
        FindingKind::SurvivingMutant => format!(
            "no test noticed {}; {} ran and passed",
            one.display_id,
            one.tests_run.map_or_else(
                || "the target".to_owned(),
                |count| {
                    if count == 1 {
                        "1 test".to_owned()
                    } else {
                        format!("{count} tests")
                    }
                }
            )
        ),
        FindingKind::WaitedMutant => format!(
            "this machine stopped waiting for {} twice, so the run established nothing about \
             it. A bound that expired is a fact about the machine, not the mutant. With a \
             step allowance, the bound ends only a process that raised no step for a whole \
             window, which is a wait rather than a loop; without one, give it one so a mutant \
             that spins is stopped by a count instead",
            one.display_id
        ),
        FindingKind::StepLimitReachedMutant => format!(
            "{} reached the configured per-process guard-take limit; that establishes where \
             this execution stopped, not that the mutation cannot terminate",
            one.display_id
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
        FindingKind::ErroredMutant => match &one.start_failure {
            Some(cause) => format!(
                "the harness never started on {}: {}, so nothing about the tests was established",
                one.display_id,
                cause.sentence()
            ),
            None => format!(
                "the harness itself failed on {} with exit {}, so nothing about the tests was \
                 established",
                one.display_id, one.exit_code
            ),
        },
        FindingKind::UnreachedMutant => format!(
            "no measured test reaches {}: the mutation lives in code the tests never execute",
            one.display_id
        ),
        FindingKind::DischargedMutant => format!(
            "every target that could have noticed {} was removed by a proof, so no test could \
             have: the mutation is in code the tests run and never observe",
            one.display_id
        ),
        FindingKind::NotRunMutant
        | FindingKind::StaleExpectation
        | FindingKind::UnmatchedExpectation
        | FindingKind::UnmatchedSkip => format!(
            "{} was never executed and nothing cancelled the run",
            one.display_id
        ),
    }
}

/// One length as a count.
/// A catalog larger than a `u32` is one no run could hold in memory to begin with.
///
/// # Errors
/// Refuses a host collection whose exact cardinality does not fit the durable run counter.
pub fn count(value: usize) -> Result<u32, SessionError> {
    u32::try_from(value).map_err(|_outside_range| SessionError::RunCountTooLarge { count: value })
}

/// What a run needs beyond the session itself.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// How long one execution may take before it is retried serially.
    /// The claims to verify.
    pub expectations: &'a [Expectation],
    /// The machine, which a confirming retry takes to itself.
    pub quiet: &'a Quiet,
    /// The tree the equivalence layer builds and mutates, when a run asks it.
    /// `None` asks nothing.
    pub equivalence: Option<&'a Equivalence<'a>>,
    /// How many mutants to measure at once.
    pub jobs: Jobs,
    /// Further arguments for the harness.
    pub args: &'a [String],
    /// Which part of the catalog this run is about.
    /// `None` is all of it.
    pub shard: Option<Shard>,
    /// Where what earlier runs of this exact tree established is kept, and this run's own name.
    /// `None` establishes everything afresh.
    pub outcomes: Option<Reusing<'a>>,
    /// Which of the catalog's mutants this run is about.
    /// `None` is every one the shard holds.
    pub filter: Option<&'a Filter>,
    /// Stop at the first finding rather than measuring the rest.
    pub fail_fast: bool,
}

/// Which of a catalog's mutants a run is about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Rules by name.
    /// Empty selects every rule.
    pub rules: Vec<String>,
    /// Families by name.
    /// Empty selects every family.
    pub families: Vec<String>,
    /// Rules never to select.
    pub skip_rules: Vec<String>,
    /// Families never to select.
    pub skip_families: Vec<String>,
    /// Paths, each with the lines of it the filter is about.
    /// Empty selects every file.
    pub files: Vec<(String, Option<(u32, u32)>)>,
    /// The identities, or prefixes of them, this run is about.
    /// `None` where nothing named any, which selects every mutant; a list that names none selects none, because a source of identities that came up empty is an answer rather than the absence of a question.
    pub ids: Option<Vec<String>>,
}

impl Filter {
    /// Whether the filter says anything at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rules.is_empty()
            && self.families.is_empty()
            && self.skip_rules.is_empty()
            && self.skip_families.is_empty()
            && self.files.is_empty()
            && self.ids.is_none()
    }

    /// Whether this run is about `mutant`, which sits at `line` inside `item`.
    #[must_use]
    pub fn selects(&self, mutant: &Mutant, line: u32, item: Option<&str>) -> bool {
        let rule = mutant.candidate.rule.name;
        let family = mutant.candidate.rule.family.name();
        if self.skip_rules.iter().any(|one| one == rule) {
            return false;
        }
        if self.skip_families.iter().any(|one| one == family) {
            return false;
        }
        if !self.rules.is_empty() && !self.rules.iter().any(|one| one == rule) {
            return false;
        }
        if !self.families.is_empty() && !self.families.iter().any(|one| one == family) {
            return false;
        }
        if let Some(ids) = &self.ids
            && !ids.iter().any(|selector| {
                Locator::parse(selector).map_or_else(
                    || mutant.id.as_str().starts_with(selector.as_str()),
                    |name| name.describes(mutant, item, line),
                )
            })
        {
            return false;
        }
        if self.files.is_empty() {
            return true;
        }
        self.files.iter().any(|(path, lines)| {
            mutant.candidate.path == *path
                && lines.is_none_or(|(from, to)| (from..=to).contains(&line))
        })
    }
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
    /// The records an answer is carried across an edit by (ADR 0041).
    pub carried: &'a crate::carry::Store,
    /// Which target last killed each mutant, which decides only the order targets are asked in.
    pub killers: &'a crate::killers::Killers,
}

/// The part of a catalog one run answers for: the part a shard holds, and whether the run's own selection narrowed the catalog to some of the project's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    /// The part of the catalog this run holds, when it holds only one.
    pub shard: Option<Shard>,
    /// Whether the catalog was built from only the files this run selected, such as a change set, so a claim on another file was never asked about.
    pub narrowed: bool,
}

impl Scope {
    /// The part a run answers for: the part `shard` holds, of a catalog its own selection did or did not narrow.
    #[must_use]
    pub const fn of(shard: Option<Shard>, narrowed: bool) -> Self {
        Self { shard, narrowed }
    }

    /// A run that answers for the whole catalog it was built from.
    pub const WHOLE: Self = Self {
        shard: None,
        narrowed: false,
    };
}

/// One part of a catalog, for a run that shares the work with others.
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

/// Why some reports are not every part of one catalog, each once.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PartsError {
    /// A part is absent.
    #[error("shard {index}/{of} is missing, so these reports are not a whole")]
    Missing {
        /// The absent part, from one.
        index: u32,
        /// How many parts there are.
        of: u32,
    },
    /// A part is offered more than once.
    #[error("shard {index}/{of} is in more than one of these reports")]
    Duplicate {
        /// The repeated part, from one.
        index: u32,
        /// How many parts there are.
        of: u32,
    },
    /// Two parts divide the catalog differently.
    #[error("shard {index}/{actual} divides the catalog differently from /{expected}")]
    Denominator {
        /// The part that disagrees.
        index: u32,
        /// How many parts the first said there are.
        expected: u32,
        /// How many parts this one says there are.
        actual: u32,
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
        let index = index.trim().parse::<u32>().map_err(|_error| malformed())?;
        let of = of.trim().parse::<u32>().map_err(|_error| malformed())?;
        if of == 0 || index == 0 || index > of {
            return Err(ShardError::OutOfRange { index, of });
        }
        Ok(Self { index, of })
    }

    /// The parts in order, when they are every part of one catalog, each once.
    ///
    /// # Errors
    /// See [`PartsError`].
    pub fn every_part<T>(parts: impl IntoIterator<Item = (Self, T)>) -> Result<Vec<T>, PartsError> {
        let mut by_index: std::collections::BTreeMap<u32, T> = std::collections::BTreeMap::new();
        let mut of = None;
        for (shard, part) in parts {
            let expected = *of.get_or_insert(shard.of);
            if shard.of != expected {
                return Err(PartsError::Denominator {
                    index: shard.index,
                    expected,
                    actual: shard.of,
                });
            }
            if by_index.insert(shard.index, part).is_some() {
                return Err(PartsError::Duplicate {
                    index: shard.index,
                    of: expected,
                });
            }
        }
        let Some(of) = of else {
            return Ok(Vec::new());
        };
        match (1..=of).find(|index| !by_index.contains_key(index)) {
            Some(index) => Err(PartsError::Missing { index, of }),
            None => Ok(by_index.into_values().collect()),
        }
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
/// # Errors
/// Returns what the engine could not do.
/// A mutant the engine refuses to execute is recorded as errored rather than ending the run.
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
    let places: Vec<&Mutant> = accepted
        .iter()
        .filter_map(|index| session.catalog().by_index(*index))
        .collect();
    let (places, mut unselected) = narrowed(session, places, options.filter);
    for mutant in session.catalog().mutants().iter().filter(|mutant| {
        options.shard.is_none_or(|shard| shard.holds(mutant.index))
            && !session.was_validated(mutant.index)
    }) {
        if filter_selects(session, mutant, options.filter) {
            return Err(EngineError::from(SessionError::UnknownMutant {
                message: format!(
                    "{} was not compiled by this prepared session; prepare with a validation filter that includes it",
                    mutant.display_id
                ),
            }));
        }
        session.trace().select(crate::trace::SelectRecord {
            mutant: mutant.display_id.to_string(),
            reason: NotRunReason::Unselected.name().to_owned(),
        });
        unselected.push(unexecuted(mutant, NotRunReason::Unselected));
    }
    let width = Width {
        asked: options.jobs,
        used: options.jobs.resolve(),
    };
    observer.starting(count(places.len())?, width);
    let judged = if width.used == 1 {
        serially(session, &places, options, (cancel, observer))?
    } else {
        pool::judge(session, &places, options, (cancel, observer, width.used))?
    };
    observer.finished(started.elapsed());
    let mut judged = judged;
    judged.extend(unselected);
    judged.sort_by_key(|one| one.index);
    if let Some(asking) = options.equivalence {
        equivalence(session, asking, &mut judged, cancel)?;
    }
    let interrupted = judged
        .iter()
        .any(|one| one.not_run_reason == Some(NotRunReason::Interrupted));
    Ok(Run {
        judged,
        expectations: Vec::new(),
        skipped: session.skips().iter().try_fold(0u32, |total, skip| {
            total
                .checked_add(skip.count)
                .ok_or(SessionError::RunCountOverflow)
        })?,
        refused: count(session.rejections().len())?,
        claims: session.claims().to_vec(),
        interrupted: interrupted || cancel.is_cancelled(),
        shard: options.shard,
        duration: started.elapsed(),
        width,
    })
}

/// The mutants a claim names, and where the first has moved to since the claim was written.
#[derive(Debug, thiserror::Error)]
enum AddressError {
    #[error(transparent)]
    Resolve(#[from] EngineError),
    #[error("the claim names no mutant")]
    MissingLocator,
    #[error(transparent)]
    Locate(#[from] LocateError),
}

fn addressed<'s>(
    session: &'s Session,
    expectation: &Expectation,
) -> Result<(Vec<&'s Mutant>, Option<Standing>), AddressError> {
    if let Some(id) = &expectation.id {
        return session
            .resolve(id)
            .map(|mutant| (vec![mutant], None))
            .map_err(AddressError::from);
    }
    let Some(locator) = &expectation.locator else {
        return Err(AddressError::MissingLocator);
    };
    let mutants = session.locate_all(locator)?;
    let moved = locator.line.zip(mutants.first()).and_then(|(from, first)| {
        let to = session.position(first)?.line;
        (to != from).then_some(Standing::Moved { from, to })
    });
    Ok((mutants, moved))
}

/// What a claim that applies here and names `mutants` stands as, marking the rows it accounts for: how many it was resolved against, the one that decided it, and its standing.
fn decided(
    judged: &mut [Judged],
    expectation: &Expectation,
    shard: Option<Shard>,
    (mutants, moved): (Vec<&Mutant>, Option<Standing>),
) -> Result<(u32, Option<String>, Standing), SessionError> {
    let every: Vec<String> = mutants.iter().map(|mutant| mutant.id.to_string()).collect();
    let ids: Vec<String> = mutants
        .iter()
        .filter(|mutant| shard.is_none_or(|part| part.holds(mutant.index)))
        .map(|mutant| mutant.id.to_string())
        .filter(|id| decided_here(judged, id))
        .collect();
    let (named, standing) = if ids.is_empty() {
        (None, Standing::Unjudged)
    } else {
        standing_of(judged, expectation.outcome, &ids)
    };
    let standing = match standing {
        Standing::Met => moved.unwrap_or(Standing::Met),
        held @ (Standing::Moved { .. }
        | Standing::Stale { .. }
        | Standing::Unmatched { .. }
        | Standing::Unjudged
        | Standing::Inapplicable { .. }) => held,
    };
    if matches!(standing, Standing::Met | Standing::Moved { .. }) {
        for one in judged.iter_mut().filter(|one| ids.contains(&one.id)) {
            one.expected = true;
        }
    }
    let covered = u32::try_from(every.len()).map_err(|_outside_range| {
        SessionError::ExpectationCoverageTooLarge { count: every.len() }
    })?;
    Ok((covered, named, standing))
}

/// What a claim the catalog does not answer for stands as: not judged here where it names something in a file no unit read, unjudged where a narrowed run left its file out, and otherwise unmatched.
fn unresolved(
    session: &Session,
    expectation: &Expectation,
    narrowed: bool,
    why: &AddressError,
) -> Standing {
    if uncompiled(session, expectation) {
        Standing::Inapplicable {
            because: Unheld::NotCompiled,
        }
    } else if narrowed && !scanned(session, expectation) {
        Standing::Unjudged
    } else {
        Standing::Unmatched {
            why: why.to_string(),
        }
    }
}

/// Whether a claim names something in a file no unit of this build read, found by walking that file as discovery walks the ones it did.
fn uncompiled(session: &Session, expectation: &Expectation) -> bool {
    expectation
        .locator
        .as_ref()
        .is_some_and(|locator| session.unread(locator) == crate::session::Unread::Named)
}

/// Whether the file a claim names is one this run's catalog was built from rather than one its selection left out; a claim by identity names no file, so a narrowed run cannot say.
fn scanned(session: &Session, expectation: &Expectation) -> bool {
    expectation.locator.as_ref().is_some_and(|locator| {
        session.files().iter().any(|file| {
            file.path == locator.path
                && file.whole_file != Some(crate::syntax::SkipReason::Excluded)
        })
    })
}

/// Whether this run decided the mutation `id`: a row it did not leave out, stop short of, or never get to.
fn decided_here(judged: &[Judged], id: &str) -> bool {
    judged.iter().find(|one| one.id == id).is_none_or(|one| {
        !(one.outcome == Outcome::NotRun
            && matches!(
                one.not_run_reason,
                Some(
                    NotRunReason::Unselected
                        | NotRunReason::StoppedEarly
                        | NotRunReason::Interrupted
                )
            ))
    })
}

/// What the run says about every mutant one claim names, and which of them decided it.
fn standing_of(judged: &[Judged], expected: Outcome, ids: &[String]) -> (Option<String>, Standing) {
    for id in ids {
        let Some(one) = judged.iter().find(|one| one.id == *id) else {
            return (
                Some(id.clone()),
                Standing::Unmatched {
                    why: "the mutant is in the catalog and the run did not reach it".to_owned(),
                },
            );
        };
        if one.outcome != expected {
            return (
                Some(id.clone()),
                Standing::Stale {
                    actual: one.outcome,
                },
            );
        }
    }
    (ids.first().cloned(), Standing::Met)
}

/// Asks the compiler whether each survivor's mutation is one it renders at all.
fn equivalence(
    session: &Session,
    asking: &Equivalence<'_>,
    judged: &mut [Judged],
    cancel: &Cancel,
) -> Result<(), EngineError> {
    let survivors: Vec<usize> = judged
        .iter()
        .enumerate()
        .filter(|(_, one)| one.outcome == Outcome::Survived)
        .map(|(at, _)| at)
        .collect();
    if survivors.is_empty() {
        return Ok(());
    }
    let phase = session.trace().phase("equivalence");
    let mut prover =
        crate::equivalence::Prover::open(asking.root, &asking.options, cancel, session.trace())?;
    for at in survivors {
        if cancel.is_cancelled() {
            break;
        }
        let Some(one) = judged.get(at) else {
            continue;
        };
        let Some(mutant) = session.catalog().by_index(one.index) else {
            continue;
        };
        let answer = prover.identical(&mutant.candidate, cancel)?;
        session.trace().identical(crate::trace::IdenticalRecord {
            index: one.index,
            identity: answer.name().to_owned(),
            detail: match answer {
                crate::equivalence::artifacts::Identity::NotEstablished(why) => {
                    Some(why.to_owned())
                }
                crate::equivalence::artifacts::Identity::Identical
                | crate::equivalence::artifacts::Identity::Differs => None,
            },
        });
        if let Some(one) = judged.get_mut(at) {
            one.identical = match answer {
                crate::equivalence::artifacts::Identity::Identical => CodegenIdentity::Identical,
                crate::equivalence::artifacts::Identity::Differs => CodegenIdentity::Different,
                crate::equivalence::artifacts::Identity::NotEstablished(_) => {
                    CodegenIdentity::NotEstablished
                }
            };
        }
    }
    prover.close()?;
    phase.end();
    Ok(())
}

/// What the equivalence layer needs: the tree the user wrote, and how it is built.
#[derive(Debug)]
pub struct Equivalence<'a> {
    /// The source root, which is what a person would build.
    pub root: &'a std::path::Path,
    /// How the tree is copied, which cargo builds it, and what it is compiled as.
    pub options: crate::equivalence::ProveOptions,
}

/// How many mutants a run measures at once, as a person asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jobs {
    /// Exactly this many.
    Count(std::num::NonZeroUsize),
    /// As many as the machine has, capped at [`DEFAULT_JOBS`], which is what a machine a person is also using wants.
    Auto,
    /// As many as the machine has, which is what a CI runner doing nothing else wants.
    All,
}

/// Why a text is not a number of jobs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum JobsError {
    /// Zero jobs would measure nothing, and `auto` says what a zero used to mean.
    #[error(
        "0 jobs would measure nothing; write `auto` for as many as the machine has, capped at {DEFAULT_JOBS}, or `all` for every one"
    )]
    Zero,
    /// The text is neither a count nor a word this release knows.
    #[error("{text:?} is not a number of jobs; write a count, `auto`, or `all`")]
    Unknown {
        /// What was written.
        text: String,
    },
}

impl JobsError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Zero | Self::Unknown { .. } => crate::error::CONFIG_INVALID,
        }
    }
}

impl Jobs {
    /// The number of jobs `text` names: a count, `auto`, or `all`.
    ///
    /// # Errors
    /// See [`JobsError`].
    pub fn parse(text: &str) -> Result<Self, JobsError> {
        match text {
            "auto" => Ok(Self::Auto),
            "all" => Ok(Self::All),
            _ => match text.parse::<usize>() {
                Ok(count) => Self::count(count),
                Err(_not_a_count) => Err(JobsError::Unknown {
                    text: text.to_owned(),
                }),
            },
        }
    }

    /// Exactly `count` jobs.
    ///
    /// # Errors
    /// [`JobsError::Zero`] for zero.
    pub fn count(count: usize) -> Result<Self, JobsError> {
        std::num::NonZeroUsize::new(count)
            .map(Self::Count)
            .ok_or(JobsError::Zero)
    }

    /// How many mutants are measured at once on a machine that offers `available` processors.
    #[must_use]
    pub fn resolve_on(self, available: usize) -> usize {
        match self {
            Self::Count(count) => count.get(),
            Self::Auto => available.clamp(1, DEFAULT_JOBS),
            Self::All => available.max(1),
        }
    }

    /// How many mutants are measured at once on this machine; one where it cannot say how many processors it has.
    #[must_use]
    pub fn resolve(self) -> usize {
        match std::thread::available_parallelism() {
            Ok(cores) => self.resolve_on(cores.get()),
            Err(_unavailable) => self.resolve_on(1),
        }
    }

    /// How a person writes it.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Count(count) => count.to_string(),
            Self::Auto => "auto".to_owned(),
            Self::All => "all".to_owned(),
        }
    }
}

impl serde::Serialize for Jobs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Count(count) => serializer
                .serialize_u64(u64::try_from(count.get()).map_err(serde::ser::Error::custom)?),
            Self::Auto | Self::All => serializer.serialize_str(&self.name()),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Jobs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Written;
        impl serde::de::Visitor<'_> for Written {
            type Value = Jobs;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a count of jobs, `auto`, or `all`")
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Jobs, E> {
                let count = usize::try_from(value).map_err(E::custom)?;
                Jobs::count(count).map_err(E::custom)
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Jobs, E> {
                let count = usize::try_from(value).map_err(E::custom)?;
                Jobs::count(count).map_err(E::custom)
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Jobs, E> {
                Jobs::parse(value).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(Written)
    }
}

/// The most mutants a run measures at once when nobody says.
pub const DEFAULT_JOBS: usize = 4;

/// Judges every mutant on the calling thread, in catalog order.
fn serially<O: Observer>(
    session: &Session,
    places: &[&Mutant],
    options: &Options<'_>,
    watching: (&Cancel, &mut O),
) -> Result<Vec<Judged>, EngineError> {
    let (cancel, observer) = watching;
    let total = count(places.len())?;
    let mut judged = Vec::with_capacity(places.len());
    let mut stopped = false;
    for (position, mutant) in places.iter().enumerate() {
        if stopped {
            judged.push(unexecuted(mutant, NotRunReason::StoppedEarly));
            continue;
        }
        if cancel.is_cancelled() {
            judged.push(unexecuted(mutant, NotRunReason::Interrupted));
            continue;
        }
        observer.started(mutant);
        let mut one = one_mutant(session, mutant, options, cancel)?;
        route(session, mutant, &mut one);
        let completed = count(position)?
            .checked_add(1)
            .ok_or(SessionError::RunCountOverflow)?;
        observer.judged(&one, completed, total);
        stopped = options.fail_fast && stops(&one);
        judged.push(one);
    }
    Ok(judged)
}

/// What one mutant is: what an earlier run of this exact tree established, or what this run measures.
fn one_mutant(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    cancel: &Cancel,
) -> Result<Judged, EngineError> {
    if let Some(one) = reuse(session, mutant, options, cancel)? {
        return Ok(one);
    }
    let (established, asked) = execute(session, mutant, options, cancel)?;
    keep(mutant, options, (&established, &asked))?;
    carry(session, mutant, (options, cancel), (&established, &asked))?;
    Ok(established)
}

/// Measuring several mutants at once, and delivering each as it finishes.
mod pool {
    use std::sync::mpsc::{SyncSender, sync_channel};
    use std::sync::{Arc, Mutex};

    use super::{
        EngineError, Judged, Mutant, NotRunReason, Observer, Options, Session, one_mutant, route,
        unexecuted,
    };
    use crate::runner::Cancel;
    use crate::workspace::SessionError;

    /// The sole owner of one scoped worker.
    /// Consuming `join` is the only successful way out of the scope, so a panic is an engine fact rather than a detached background failure.
    struct JoinedWorker<'scope>(std::thread::ScopedJoinHandle<'scope, ()>);

    impl<'scope> JoinedWorker<'scope> {
        fn launch(
            scope: &'scope std::thread::Scope<'scope, '_>,
            ordinal: usize,
            work: impl FnOnce() + Send + 'scope,
        ) -> Result<Self, EngineError> {
            std::thread::Builder::new()
                .name(format!("rust-mutants-worker-{ordinal}"))
                .spawn_scoped(scope, work)
                .map(Self)
                .map_err(|source| {
                    SessionError::WorkerStartFailed {
                        worker: ordinal,
                        source,
                    }
                    .into()
                })
        }

        fn join(self) -> Result<(), EngineError> {
            self.0
                .join()
                .map_err(|_panic| SessionError::WorkerPanicked.into())
        }
    }

    /// Which catalog position a worker owns next, and whether fail-fast closed the queue.
    /// Both fields move under one lock so no worker can claim after the stop transition.
    struct WorkState {
        next: usize,
        stopped: bool,
    }

    impl WorkState {
        const fn claim(&mut self, len: usize) -> Option<usize> {
            if self.stopped || self.next >= len {
                return None;
            }
            let at = self.next;
            self.next = match at.checked_add(1) {
                Some(next) => next,
                None => {
                    self.stopped = true;
                    return None;
                }
            };
            Some(at)
        }
    }

    /// What a worker hands the coordinator.
    enum Delivery {
        /// A worker claimed the mutant at this position and started it.
        Started(usize),
        /// A worker finished the mutant at this position.
        Judged(usize, Box<Judged>),
        /// A worker could not go on, and neither can the run.
        Failed(Box<EngineError>),
    }

    fn deliver(sender: &SyncSender<Delivery>, delivery: Delivery) -> bool {
        sender.send(delivery).is_ok()
    }

    fn notify_failure(sender: &SyncSender<Delivery>, failure: Delivery) {
        match sender.send(failure) {
            Ok(()) => {}
            Err(std::sync::mpsc::SendError(undelivered)) => drop(undelivered),
        }
    }

    struct WorkerContext<'a> {
        session: &'a Session,
        places: &'a [&'a Mutant],
        options: &'a Options<'a>,
        cancel: &'a Cancel,
        state: Arc<Mutex<WorkState>>,
    }

    fn worker_loop(context: &WorkerContext<'_>, sender: &SyncSender<Delivery>) {
        loop {
            let at = match context.state.lock() {
                Ok(mut state) => state.claim(context.places.len()),
                Err(_poisoned) => {
                    let failure =
                        Delivery::Failed(Box::new(SessionError::WorkerStatePoisoned.into()));
                    notify_failure(sender, failure);
                    return;
                }
            };
            let Some(at) = at else {
                return;
            };
            let Some(mutant) = context.places.get(at) else {
                return;
            };
            if context.cancel.is_cancelled() || !deliver(sender, Delivery::Started(at)) {
                return;
            }
            let delivery =
                match one_mutant(context.session, mutant, context.options, context.cancel) {
                    Ok(mut one) => {
                        route(context.session, mutant, &mut one);
                        Delivery::Judged(at, Box::new(one))
                    }
                    Err(error) => Delivery::Failed(Box::new(error)),
                };
            if !deliver(sender, delivery) {
                return;
            }
        }
    }

    struct Coordinator<'a, O> {
        places: &'a [&'a Mutant],
        options: &'a Options<'a>,
        cancel: &'a Cancel,
        observer: &'a mut O,
        state: Arc<Mutex<WorkState>>,
        done: Vec<Option<Judged>>,
        failure: Option<EngineError>,
        completed: u32,
        total: u32,
        stopped: bool,
    }

    impl<O: Observer> Coordinator<'_, O> {
        fn fail(&mut self, error: EngineError) {
            self.cancel.cancel();
            if self.failure.is_none() {
                self.failure = Some(error);
            }
        }

        fn stop_claiming(&mut self) {
            let poisoned = match self.state.lock() {
                Ok(mut state) => {
                    state.stopped = true;
                    false
                }
                Err(_poisoned) => true,
            };
            if poisoned {
                self.fail(SessionError::WorkerStatePoisoned.into());
            }
        }

        fn accept(&mut self, delivery: Delivery) {
            match delivery {
                Delivery::Started(at) => {
                    if let Some(mutant) = self.places.get(at) {
                        self.observer.started(mutant);
                    }
                }
                Delivery::Judged(at, one) => self.accept_judged(at, *one),
                Delivery::Failed(error) => self.fail(*error),
            }
        }

        fn accept_judged(&mut self, at: usize, one: Judged) {
            let Some(completed) = self.completed.checked_add(1) else {
                self.fail(SessionError::CompletedCountExhausted.into());
                return;
            };
            self.completed = completed;
            self.observer.judged(&one, completed, self.total);
            self.stopped |= self.options.fail_fast && super::stops(&one);
            if let Some(place) = self.done.get_mut(at) {
                *place = Some(one);
            }
            if self.stopped {
                self.stop_claiming();
            }
        }

        fn finish(self) -> Result<Vec<Judged>, EngineError> {
            if let Some(error) = self.failure {
                return Err(error);
            }
            let unreached = if self.stopped {
                NotRunReason::StoppedEarly
            } else {
                NotRunReason::Interrupted
            };
            Ok(self
                .places
                .iter()
                .zip(self.done)
                .map(|(mutant, one)| one.unwrap_or_else(|| unexecuted(mutant, unreached)))
                .collect())
        }
    }

    /// Judges every mutant with `jobs` of them in flight, delivering each as it finishes.
    pub(super) fn judge<O: Observer>(
        session: &Session,
        places: &[&Mutant],
        options: &Options<'_>,
        watching: (&Cancel, &mut O, usize),
    ) -> Result<Vec<Judged>, EngineError> {
        let (asked, observer, workers) = watching;
        let stopping = asked.child();
        let cancel = &stopping;
        let total =
            u32::try_from(places.len()).map_err(|_overflow| SessionError::WorkerQueueTooLarge {
                workers: places.len(),
            })?;
        if places.is_empty() {
            return Ok(Vec::new());
        }
        let worker_count = workers.min(places.len());
        let capacity = worker_count
            .checked_mul(2)
            .ok_or(SessionError::WorkerQueueTooLarge {
                workers: worker_count,
            })?;
        let work = Arc::new(Mutex::new(WorkState {
            next: 0,
            stopped: false,
        }));
        let (sender, receiver) = sync_channel::<Delivery>(capacity);
        let mut coordinator = Coordinator {
            places,
            options,
            cancel,
            observer,
            state: Arc::clone(&work),
            done: (0..places.len()).map(|_| None).collect(),
            failure: None,
            completed: 0,
            total,
            stopped: false,
        };

        std::thread::scope(|scope| {
            let mut workers = Vec::with_capacity(worker_count);
            for ordinal in 0..worker_count {
                let sender = sender.clone();
                let context = WorkerContext {
                    session,
                    places,
                    options,
                    cancel,
                    state: Arc::clone(&work),
                };
                match JoinedWorker::launch(scope, ordinal, move || {
                    worker_loop(&context, &sender);
                }) {
                    Ok(worker) => workers.push(worker),
                    Err(error) => {
                        coordinator.fail(error);
                        coordinator.stop_claiming();
                        break;
                    }
                }
            }
            drop(sender);
            for delivery in receiver {
                coordinator.accept(delivery);
            }
            for worker in workers {
                if let Err(error) = worker.join() {
                    coordinator.fail(error);
                }
            }
        });
        coordinator.finish()
    }
}

/// Records which targets could have noticed this mutation and which of them ran.
fn route(session: &Session, mutant: &Mutant, judged: &mut Judged) {
    if let Some(reason) = judged.not_run_reason
        && session.trace().is_enabled()
    {
        session.trace().select(crate::trace::SelectRecord {
            mutant: mutant.display_id.to_string(),
            reason: reason.name().to_owned(),
        });
    }
    if judged.measured {
        return;
    }
    let decided = session.route(mutant);
    judged.route = Some(crate::report::run::route_document(&decided, Vec::new()));
    if !session.trace().is_enabled() {
        return;
    }
    let mut record = decided.record(mutant, Vec::new());
    record.reused.clone_from(&judged.source_run_id);
    session.trace().route(record);
}

/// What a caller hears while a run happens.
pub trait Observer {
    /// The run is about to judge `total` mutants, `width` of them at once.
    fn starting(&mut self, _total: u32, _width: Width) {}

    /// A mutant is about to be judged.
    fn started(&mut self, _mutant: &Mutant) {}

    /// A mutant has been judged.
    /// `completed` counts what has been delivered, of `total`.
    fn judged(&mut self, _judged: &Judged, _completed: u32, _total: u32) {}

    /// Every mutant has been judged, and the run took `duration`.
    fn finished(&mut self, _duration: Duration) {}
}

/// An observer that draws nothing, for a caller that reads the report instead.
#[derive(Debug, Default, Clone, Copy)]
pub struct Silent;

impl Observer for Silent {}

/// Why a mutant was never executed.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum NotRunReason {
    /// The coverage measurement proved no target reaches it.
    Unreached,
    /// A proof discharged every target it could have been noticed by.
    Discharged,
    /// The run was interrupted before it got there.
    Interrupted,
    /// A filter took it out of what this run was asked to measure.
    Unselected,
    /// The run stopped at the first finding, as it was asked to.
    StoppedEarly,
    /// Every test that reached it declined to measure on this machine, as the baseline's did (ADR 0043).
    Declined,
}

impl NotRunReason {
    /// The reason that answers to `name`, when one does.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// The kebab-case name a report and a recording use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::Discharged => "discharged",
            Self::Interrupted => "interrupted",
            Self::Unselected => "unselected",
            Self::StoppedEarly => "stopped-early",
            Self::Declined => "declined",
        }
    }

    /// The canonical wire name, for interfaces that take a string slice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.name()
    }
}

impl std::fmt::Display for FindingKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.pad(self.name())
    }
}

/// One mutant, executed once and — when it timed out — once more on its own before the timeout is believed.
fn execute(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    cancel: &Cancel,
) -> Result<(Judged, Vec<crate::execute::MutantResult>), EngineError> {
    let first = match options.outcomes.as_ref() {
        Some(reusing) => reusing.killers.of(mutant.id.as_str())?,
        None => None,
    };
    let recording = match options.outcomes {
        Some(_) => crate::session::Recording::Items,
        None => crate::session::Recording::Off,
    };
    let request = Request::new(mutant.id.to_string())
        .with_args(options.args.to_vec())
        .trying_first(first)
        .recording(recording);
    let judgement = session.judge(&request, options.quiet, cancel)?;
    let duration = judgement.duration();
    let retried = judgement.retried();
    let crate::session::Judgement {
        attempts,
        route,
        asked,
        executed,
        ..
    } = judgement;
    let result = attempts.into_result();
    let outcome = result.outcome();
    let tests_run = result.tests_run();
    let not_run_reason = not_run_because(&result.conclusion, &route);
    let declined = match &result.declines {
        crate::decline::Declines::Read { declined, .. } => declined.clone(),
        crate::decline::Declines::Unbelieved { .. } => Vec::new(),
    };
    let failed_tests = match &result.conclusion {
        crate::execute::MutantConclusion::DeclinedUnderTheMutant { by } => vec![by.test.clone()],
        crate::execute::MutantConclusion::NotRun
        | crate::execute::MutantConclusion::Killed
        | crate::execute::MutantConclusion::Survived
        | crate::execute::MutantConclusion::StepLimitReached { .. }
        | crate::execute::MutantConclusion::Waited
        | crate::execute::MutantConclusion::Inconclusive
        | crate::execute::MutantConclusion::Unobserved
        | crate::execute::MutantConclusion::Errored
        | crate::execute::MutantConclusion::Declined { .. } => result.failed_tests.clone(),
    };
    let judged = Judged {
        index: mutant.index,
        id: mutant.id.to_string(),
        display_id: mutant.display_id.to_string(),
        outcome,
        step_notice: result.step_notice().cloned(),
        target: result.target,
        exit_code: result.exit_code,
        start_failure: result.stopped.start_failure().cloned(),
        duration,
        tests_run,
        failed_tests,
        signal: result.signal,
        retried,
        lingered: result.lingered,
        expected: false,
        not_run_reason,
        route: Some(crate::report::run::route_document(&route, executed)),
        measured: true,
        identical: CodegenIdentity::NotMeasured,
        source_run_id: None,
        declined,
    };
    Ok((judged, asked))
}

/// What a mutant's finding is decided from, whether the row is the run's or its report's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowVerdict {
    /// What the execution says.
    pub outcome: Outcome,
    /// Why it was never executed, or measured nothing where it was, when it was not.
    pub not_run_reason: Option<NotRunReason>,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
}

/// The finding `row` raises in a run that was or was not `interrupted`, when it raises one: the one place that says which verdicts keep a run from being clean, for the run and for every reader of its report.
#[must_use]
pub const fn verdict_finding(row: RowVerdict, interrupted: bool) -> Option<FindingKind> {
    let RowVerdict {
        outcome,
        not_run_reason: reason,
        expected,
    } = row;
    match outcome {
        Outcome::Killed => None,
        Outcome::Survived if expected => None,
        Outcome::Survived => Some(FindingKind::SurvivingMutant),
        Outcome::StepLimitReached => Some(FindingKind::StepLimitReachedMutant),
        Outcome::Waited => Some(FindingKind::WaitedMutant),
        Outcome::Inconclusive => Some(FindingKind::InconclusiveMutant),
        Outcome::Errored => Some(FindingKind::ErroredMutant),
        Outcome::NotRun => match reason {
            Some(NotRunReason::Unreached) => Some(FindingKind::UnreachedMutant),
            Some(NotRunReason::Discharged) => Some(FindingKind::DischargedMutant),
            Some(
                NotRunReason::Interrupted
                | NotRunReason::Unselected
                | NotRunReason::StoppedEarly
                | NotRunReason::Declined,
            ) => None,
            None if interrupted => None,
            None => Some(FindingKind::NotRunMutant),
        },
    }
}

/// Why a mutant that was never executed, or measured nothing where it was, was not, when it was not.
fn not_run_because(
    conclusion: &crate::execute::MutantConclusion,
    route: &crate::session::Route,
) -> Option<NotRunReason> {
    if let crate::execute::MutantConclusion::Declined { .. } = conclusion {
        return Some(NotRunReason::Declined);
    }
    if conclusion.outcome() != Outcome::NotRun {
        return None;
    }
    match route {
        crate::session::Route::Unreached { .. } => Some(NotRunReason::Unreached),
        crate::session::Route::Discharged { .. } => Some(NotRunReason::Discharged),
        crate::session::Route::All { .. } | crate::session::Route::Block { .. } => {
            Some(NotRunReason::Interrupted)
        }
    }
}

/// Whether this outcome is the one a run asked to stop at the first finding stops at.
const fn stops(one: &Judged) -> bool {
    match one.outcome {
        Outcome::Killed => false,
        Outcome::Survived => !one.expected,
        Outcome::NotRun => matches!(
            one.not_run_reason,
            Some(NotRunReason::Unreached | NotRunReason::Discharged)
        ),
        Outcome::StepLimitReached | Outcome::Waited | Outcome::Inconclusive | Outcome::Errored => {
            true
        }
    }
}

/// What a filter leaves of a catalog, and what it took out.
fn narrowed<'m>(
    session: &Session,
    places: Vec<&'m Mutant>,
    filter: Option<&Filter>,
) -> (Vec<&'m Mutant>, Vec<Judged>) {
    let Some(filter) = filter.filter(|one| !one.is_empty()) else {
        return (places, Vec::new());
    };
    let mut selected = Vec::with_capacity(places.len());
    let mut left = Vec::new();
    for mutant in places {
        if filter_selects(session, mutant, Some(filter)) {
            selected.push(mutant);
        } else {
            if session.trace().is_enabled() {
                session.trace().select(crate::trace::SelectRecord {
                    mutant: mutant.display_id.to_string(),
                    reason: NotRunReason::Unselected.name().to_owned(),
                });
            }
            left.push(unexecuted(mutant, NotRunReason::Unselected));
        }
    }
    (selected, left)
}

fn filter_selects(session: &Session, mutant: &Mutant, filter: Option<&Filter>) -> bool {
    filter.filter(|one| !one.is_empty()).is_none_or(|filter| {
        let line = session.position(mutant).map_or(0, |at| at.line);
        filter.selects(mutant, line, session.item_of(mutant.index))
    })
}

/// What an earlier run of this exact tree established about this mutant, when a record answers for it.
fn reuse(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    cancel: &Cancel,
) -> Result<Option<Judged>, EngineError> {
    let Some(reusing) = options.outcomes else {
        return Ok(None);
    };
    if !reusing.keyed.usable() {
        return Ok(None);
    }
    let mutant_id = crate::id::HexDigest::try_from(mutant.id.as_str())?;
    let key = reusing.keyed.key(&mutant_id);
    let found = reusing
        .store
        .get(&key, &mutant_id)?
        .filter(|(_, record)| session.believable(mutant, record.outcome));
    if session.trace().is_enabled() {
        session.trace().cache(crate::trace::CacheRecord {
            mutant: mutant.display_id.to_string(),
            key: key.to_string(),
            hit: found.is_some(),
            source_run_id: found.as_ref().map(|(_, record)| record.run_id.clone()),
            rule: CacheRule::Exact.name().to_owned(),
            refused: None,
        });
    }
    let Some((outcome, record)) = found else {
        return carried(session, mutant, options, (reusing, cancel));
    };
    Ok(Some(remembered(
        mutant,
        outcome,
        Remembered {
            target: record.target,
            tests_run: record.tests_run,
            failed_tests: record.failed_tests,
            run_id: record.run_id,
        },
    )))
}

/// How an answer from an earlier run was looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
enum CacheRule {
    /// Under the whole compiled closure, which any edit moves.
    Exact,
    /// Under the mutation's locus, across an edit its executions never entered (ADR 0041).
    Carried,
}

impl CacheRule {
    const fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Carried => "carried",
        }
    }
}

/// What an earlier run established about one mutant, as a judgement carries it.
struct Remembered {
    target: String,
    tests_run: Option<u32>,
    failed_tests: Vec<String>,
    run_id: String,
}

fn remembered(mutant: &Mutant, outcome: Outcome, record: Remembered) -> Judged {
    Judged {
        index: mutant.index,
        id: mutant.id.to_string(),
        display_id: mutant.display_id.to_string(),
        outcome,
        step_notice: None,
        target: record.target,
        exit_code: 0,
        start_failure: None,
        duration: Duration::ZERO,
        tests_run: record.tests_run,
        failed_tests: record.failed_tests,
        signal: None,
        retried: false,
        lingered: false,
        expected: false,
        not_run_reason: None,
        route: None,
        measured: false,
        identical: CodegenIdentity::NotMeasured,
        source_run_id: Some(record.run_id),
        declined: Vec::new(),
    }
}

/// What an earlier run established about this mutation on another tree, where every premise of ADR 0041 carries it here.
fn carried(
    session: &Session,
    mutant: &Mutant,
    options: &Options<'_>,
    (reusing, cancel): (Reusing<'_>, &Cancel),
) -> Result<Option<Judged>, EngineError> {
    let Some(locus) = session.locus(mutant) else {
        return Ok(None);
    };
    let key = crate::carry::key(reusing.keyed, &locus);
    let record = reusing.carried.get(&key)?;
    let decided = match &record {
        None => None,
        Some(record) => Some(session.believing(record, mutant, (options.args, cancel))?),
    };
    if session.trace().is_enabled() {
        session.trace().cache(crate::trace::CacheRecord {
            mutant: mutant.display_id.to_string(),
            key: key.to_string(),
            hit: matches!(decided, Some(Ok(()))),
            source_run_id: record.as_ref().map(|one| one.run_id.clone()),
            rule: CacheRule::Carried.name().to_owned(),
            refused: match decided {
                Some(Err(refusal)) => Some(refusal.name().to_owned()),
                Some(Ok(())) | None => None,
            },
        });
    }
    let (Some(record), Some(Ok(()))) = (record, decided) else {
        return Ok(None);
    };
    Ok(Some(remembered(
        mutant,
        record.outcome.into(),
        Remembered {
            target: record.target,
            tests_run: record.tests_run,
            failed_tests: record.failed_tests,
            run_id: record.run_id,
        },
    )))
}

/// Records what this run established under the mutation's locus, with every execution it rests on, so a later tree that differs only where none of them went can carry it.
fn carry(
    session: &Session,
    mutant: &Mutant,
    (options, cancel): (&Options<'_>, &Cancel),
    (judged, asked): (&Judged, &[crate::execute::MutantResult]),
) -> Result<(), EngineError> {
    let Some(reusing) = options.outcomes else {
        return Ok(());
    };
    let outcome = match judged.outcome {
        Outcome::Killed => crate::outcomes::CacheOutcome::Killed,
        Outcome::Survived => crate::outcomes::CacheOutcome::Survived,
        Outcome::NotRun
        | Outcome::StepLimitReached
        | Outcome::Waited
        | Outcome::Inconclusive
        | Outcome::Errored => return Ok(()),
    };
    if cancel.is_cancelled() {
        return Ok(());
    }
    let answered = crate::session::Answered {
        keyed: reusing.keyed.clone(),
        outcome,
        target: judged.target.clone(),
        tests_run: judged.tests_run,
        failed_tests: judged.failed_tests.clone(),
        run_id: reusing.run_id.to_owned(),
    };
    let Some(record) = session.carrying(mutant, (options.args, cancel), (answered, asked))? else {
        return Ok(());
    };
    reusing.carried.put(&record)?;
    Ok(())
}

/// Records what this run established, for the next run of this exact tree.
/// Only an outcome about the mutant is kept: a run that could not decide, or that never ran, says nothing the next run could inherit, and neither does one resting on an execution in which a test declined to measure.
fn keep(
    mutant: &Mutant,
    options: &Options<'_>,
    (judged, asked): (&Judged, &[crate::execute::MutantResult]),
) -> Result<(), EngineError> {
    if let Some(reusing) = options.outcomes.as_ref()
        && judged.outcome == Outcome::Killed
    {
        reusing
            .killers
            .remember(mutant.id.as_str(), &judged.target)?;
    }
    let Some(reusing) = options.outcomes else {
        return Ok(());
    };
    let outcome = match judged.outcome {
        Outcome::Killed => crate::outcomes::CacheOutcome::Killed,
        Outcome::Survived => crate::outcomes::CacheOutcome::Survived,
        Outcome::NotRun
        | Outcome::StepLimitReached
        | Outcome::Waited
        | Outcome::Inconclusive
        | Outcome::Errored => return Ok(()),
    };
    if !reusing.keyed.usable() || !crate::decline::storable(asked) {
        return Ok(());
    }
    let mutant_id = crate::id::HexDigest::try_from(mutant.id.as_str())?;
    reusing.store.put(&crate::outcomes::Record {
        schema: crate::outcomes::SCHEMA.to_owned(),
        mutant: mutant_id,
        outcome,
        target: judged.target.clone(),
        tests_run: judged.tests_run,
        failed_tests: judged.failed_tests.clone(),
        run_id: reusing.run_id.to_owned(),
        keyed: reusing.keyed.clone(),
    })?;
    Ok(())
}

fn unexecuted(mutant: &Mutant, reason: NotRunReason) -> Judged {
    Judged {
        index: mutant.index,
        id: mutant.id.to_string(),
        display_id: mutant.display_id.to_string(),
        outcome: Outcome::NotRun,
        step_notice: None,
        target: String::new(),
        exit_code: crate::runner::EXIT_CODE_UNAVAILABLE,
        start_failure: None,
        duration: Duration::ZERO,
        tests_run: None,
        failed_tests: Vec::new(),
        signal: None,
        retried: false,
        lingered: false,
        expected: false,
        not_run_reason: Some(reason),
        route: None,
        measured: false,
        identical: CodegenIdentity::NotMeasured,
        source_run_id: None,
        declined: Vec::new(),
    }
}

/// Resolves every declared expectation against what the run established, and marks the mutants a reviewer accounted for.
///
/// A part of a sharded run judges only the mutations it holds, since the parts together hold each exactly once and the merge answers for the claim.
///
/// # Errors
/// Refuses when one expectation resolves to more mutants than the durable coverage counter can represent.
pub fn verify(
    session: &Session,
    expectations: &[Expectation],
    judged: &mut [Judged],
    scope: Scope,
) -> Result<Vec<Verified>, SessionError> {
    let Scope { shard, narrowed } = scope;
    let mut verified = Vec::with_capacity(expectations.len());
    let mut reasons: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
    for expectation in expectations {
        let resolved = addressed(session, expectation);
        let applies = session.holds(&expectation.under);
        let named: &[&Mutant] = match (&resolved, &applies) {
            (Ok((mutants, _moved)), Ok(())) => mutants,
            (Err(_), _) | (Ok(_), Err(_)) => &[],
        };
        for mutant in named {
            if let Some(first) = reasons.insert(mutant.index, expectation.name()) {
                return Err(SessionError::ExpectationsOverlap {
                    first,
                    second: expectation.name(),
                    mutant: session.position(mutant).map_or_else(
                        || mutant.display_id.to_string(),
                        |at| format!("{}@{}", mutant.display_id, at.line),
                    ),
                });
            }
        }
        let (covered, mutant, standing) = match (resolved, applies) {
            (Err(why), _) => (0, None, unresolved(session, expectation, narrowed, &why)),
            (Ok(_), Err(because)) => (0, None, Standing::Inapplicable { because }),
            (Ok(named), Ok(())) => decided(judged, expectation, shard, named)?,
        };
        verified.push(Verified {
            id: expectation.name(),
            locator: expectation.locator.clone(),
            reason: expectation.reason.clone(),
            outcome: expectation.outcome,
            mutant,
            covered,
            standing,
            under: expectation.under.clone(),
        });
    }
    Ok(verified)
}
