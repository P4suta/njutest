// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asking, of every mutation the compiler accepted, whether any test would notice it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rust_mutants::catalog::Mutant;
use rust_mutants::execute::{MutantConclusion, MutantResult};
use rust_mutants::outcome::Outcome;
use rust_mutants::session::{Observing, Request, Session};

use crate::assure::baseline::Measured;
use crate::assure::route::Route;
use crate::assure::schedule;
use crate::evidence::store;
use crate::report::Outcome as Recorded;
use crate::report::drift::Drift;
use crate::report::{Decision, Finding, FindingKind, MutantAccounting};
use crate::watch::Watch;

/// Why a mutation cannot be represented in the assurance report.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum MutationTextError {
    /// The original source bytes are not valid Rust source text.
    #[error("a mutation's original source is not valid UTF-8: {source}")]
    Original {
        /// Why the bytes are not UTF-8.
        #[source]
        source: std::str::Utf8Error,
    },
    /// The replacement source bytes are not valid Rust source text.
    #[error("a mutation's replacement source is not valid UTF-8: {source}")]
    Replacement {
        /// Why the bytes are not UTF-8.
        #[source]
        source: std::str::Utf8Error,
    },
}

/// Why a kill could not be believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unconfirmed {
    /// The same test failed on the original code: it was going to fail anyway, and the mutant had nothing to do with it.
    ControlFailed {
        /// What the control said.
        detail: String,
    },
    /// The kill did not happen the second time.
    DidNotReproduce,
}

impl Unconfirmed {
    /// One sentence a person can act on.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::ControlFailed { detail } => format!(
                "the same test failed on the original code immediately before confirmation, \
                 so the failure was not the mutation: {detail}"
            ),
            Self::DidNotReproduce => {
                "the kill did not happen the second time, so the test does not reliably \
                 notice this mutation"
                    .to_owned()
            }
        }
    }
}

/// What one mutation established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// The compiler refused it; it never ran, and it says nothing about the tests.
    Rejected {
        /// What the compiler said.
        diagnostic: String,
    },
    /// A test noticed it, and the pair agreed.
    Killed {
        /// The test that noticed.
        by: String,
    },
    /// The verified runtime guard crossed its configured boundary.
    /// This is a reproducible execution fact, but without a matched control it is not a verdict.
    StepLimitReached {
        /// The target it was running under.
        on: String,
        /// The checked first count beyond the configured allowance.
        boundary: crate::report::StepBoundary,
    },
    /// This machine stopped waiting with it active, which establishes nothing either way.
    Waited {
        /// The test that did not finish.
        on: String,
    },
    /// Every test that could notice it passed with it active.
    Survived {
        /// How its tests were chosen, which is what a reader needs to judge the claim.
        route: Route,
    },
    /// No measured test reaches it: the mutation lives in code the tests never execute.
    Unreached,
    /// The tests ran the position and nothing noticed, and the compiler renders the mutation identically to the code it mutates: no test could have noticed.
    Equivalent {
        /// How its tests were chosen, which is the premise that separates this from untested code.
        route: Route,
    },
    /// A test noticed it and the pair did not agree, so nothing was established either way.
    Unconfirmed {
        /// The test.
        on: String,
        /// Which half of the pair disagreed.
        why: Unconfirmed,
    },
    /// The harness itself failed, so the mutation was not measured.
    Errored {
        /// The test that could not be run.
        on: String,
        /// What happened.
        detail: String,
    },
}

impl Disposition {
    /// What the run established, which is the one thing a report records this as.
    #[must_use]
    pub const fn outcome(&self) -> Recorded {
        match self {
            Self::Rejected { .. } => Recorded::CompileRejected,
            Self::Killed { .. } => Recorded::Killed,
            Self::StepLimitReached { .. } => Recorded::StepLimitReached,
            Self::Waited { .. } => Recorded::Waited,
            Self::Survived { .. } => Recorded::Survived,
            Self::Unreached => Recorded::Unreached,
            Self::Equivalent { .. } => Recorded::Equivalent,
            Self::Unconfirmed { .. } => Recorded::Unconfirmed,
            Self::Errored { .. } => Recorded::Errored,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.outcome().name()
    }

    /// What the run established, with the target it was established against.
    #[must_use]
    pub fn decided(&self) -> crate::report::Decided {
        match self {
            Self::Rejected { .. } => crate::report::Decided::CompileRejected,
            Self::Killed { by } => crate::report::Decided::Killed { by: by.clone() },
            Self::StepLimitReached { on, boundary } => crate::report::Decided::StepLimitReached {
                on: on.clone(),
                boundary: *boundary,
            },
            Self::Waited { on } => crate::report::Decided::Waited { on: on.clone() },
            Self::Survived { .. } => crate::report::Decided::Survived,
            Self::Unreached => crate::report::Decided::Unreached,
            Self::Equivalent { .. } => crate::report::Decided::Equivalent,
            Self::Unconfirmed { on, .. } => crate::report::Decided::Unconfirmed { on: on.clone() },
            Self::Errored { on, .. } => crate::report::Decided::Errored { on: on.clone() },
        }
    }

    /// Who decided it, which is what stands behind the verdict it feeds.
    ///
    /// Read through the outcome rather than spelled again here.
    /// This mapping used to exist three times, and they disagreed: two of them called a timeout a detection while the finding beside them said an expired budget establishes nothing about the mutation.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        self.outcome().decision()
    }

    /// The test that decided it, when one did.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn decided_by(&self) -> Option<&str> {
        match self {
            Self::Killed { by } => Some(by),
            Self::StepLimitReached { on, .. }
            | Self::Waited { on }
            | Self::Unconfirmed { on, .. }
            | Self::Errored { on, .. } => Some(on),
            Self::Rejected { .. }
            | Self::Survived { .. }
            | Self::Unreached
            | Self::Equivalent { .. } => None,
        }
    }
}

/// One mutant and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// The dense canonical catalog position.
    pub catalog_index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The rule that proposed it.
    pub rule: String,
    /// The item it sits in, which is how a reader names it after editing the file.
    pub item: String,
    /// The bytes the edit replaces.
    pub original: String,
    /// The bytes it puts there instead.
    pub replacement: String,
    /// Where it is, when the catalog could say.
    pub position: Option<crate::report::Position>,
    /// What was established.
    pub disposition: Disposition,
    /// Which targets could have noticed it, and what removed the rest.
    /// `None` where the run never asked, which is a mutation the compiler refused.
    pub routing: Option<crate::report::Routing>,
    /// The run that established it, when it was not this one.
    pub source_run_id: Option<String>,
    /// What the controls run to confirm it established about each target's baseline reach.
    pub observed: Vec<Drift>,
}

/// What the mutation phase established, whole.
#[derive(Debug, Clone, Default)]
pub struct Mutation {
    /// Every mutant, in catalog order.
    pub judged: Vec<Judged>,
    /// What every phase of the engine skipped, by reason, for the report's limitations.
    pub skips: BTreeMap<String, u64>,
    /// Whether each target the baseline measured held its reach on a control, one record each.
    pub drift: Vec<Drift>,
    /// The SHA-256 of each file the catalog's mutants were read from, as the catalog read it.
    pub sources: BTreeMap<String, rust_mutants::id::HexDigest>,
    /// How many dispositions resting on each moved target were decided again against it (ADR 0036).
    pub repaired: BTreeMap<String, usize>,
}

impl Mutation {
    /// The counts, folded from the dispositions.
    ///
    /// # Errors
    /// Returns [`crate::report::CountError`] rather than inventing a terminal value when a durable counter cannot hold the exact census.
    pub fn accounting(
        &self,
        accepted: &BTreeSet<String>,
    ) -> Result<MutantAccounting, crate::report::CountError> {
        let mut counts = MutantAccounting {
            cataloged: u32::try_from(self.judged.len()).map_err(|_outside_wire_range| {
                crate::report::CountError::Width {
                    ledger: "mutation catalog",
                    count: self.judged.len(),
                }
            })?,
            ..MutantAccounting::default()
        };
        for judged in &self.judged {
            counts.observers.counted(judged.disposition.decision())?;
            if answered_by(judged, accepted) {
                increment("accepted mutants", &mut counts.accepted)?;
            }
            match &judged.disposition {
                Disposition::Rejected { .. } => {
                    increment("rejected mutants", &mut counts.rejected)?;
                }
                Disposition::Killed { .. } => {
                    increment("executed mutants", &mut counts.executed)?;
                    increment("killed mutants", &mut counts.killed)?;
                    if judged.source_run_id.is_some() {
                        increment("reused killed mutants", &mut counts.reused_killed)?;
                    }
                }
                Disposition::StepLimitReached { .. } => {
                    increment("executed mutants", &mut counts.executed)?;
                    increment("step-limited mutants", &mut counts.step_limit_reached)?;
                }
                Disposition::Waited { .. } => {
                    increment("executed mutants", &mut counts.executed)?;
                    increment("waited mutants", &mut counts.waited)?;
                }
                Disposition::Survived { .. } => {
                    increment("executed mutants", &mut counts.executed)?;
                    increment("surviving mutants", &mut counts.survived)?;
                    if judged.source_run_id.is_some() {
                        increment("reused surviving mutants", &mut counts.reused_survived)?;
                    }
                }
                Disposition::Unreached => {
                    increment("unreached mutants", &mut counts.unreached)?;
                }
                Disposition::Equivalent { .. } => {
                    increment("equivalent mutants", &mut counts.equivalent)?;
                }
                Disposition::Unconfirmed { .. } | Disposition::Errored { .. } => {
                    increment("executed mutants", &mut counts.executed)?;
                }
            }
        }
        Ok(counts)
    }

    /// What a reader has to act on: a mutation nobody noticed, a pair that did not agree, a harness that failed.
    #[must_use]
    pub fn findings(&self, accepted: &BTreeSet<String>) -> Vec<Finding> {
        self.judged
            .iter()
            .filter(|judged| !answered_by(judged, accepted))
            .filter_map(finding_of)
            .collect()
    }
}

/// Adds one exact fact to a durable report counter.
fn increment(field: &'static str, count: &mut u32) -> Result<(), crate::report::CountError> {
    *count = count
        .checked_add(1)
        .ok_or(crate::report::CountError::Overflow { field })?;
    Ok(())
}

/// Whether a reviewer's acceptance answers for this mutation.
pub(super) fn answered_by(judged: &Judged, accepted: &BTreeSet<String>) -> bool {
    judged.disposition.outcome().review_answerable() && accepted.contains(&judged.id)
}

/// What a person watching a run wants to read as each answer lands.
///
/// An identity says nothing to anybody watching; it is a name for going back to one afterwards.
/// Where it is, what was tried, and what came of it is what somebody is waiting to learn.
fn watching(judged: &Judged) -> String {
    let named = if judged.item.is_empty() {
        judged.path.clone()
    } else {
        format!("{}:{}", judged.path, judged.item)
    };
    format!("{named} {} {}", judged.rule, judged.disposition.name())
}

/// The finding one disposition raises, if it raises one.
/// What a survivor's finding says, which is not the same sentence when nothing ran.
fn survived(judged: &Judged, route: &Route) -> String {
    let at = format!(
        "{} at {}:{}",
        judged.rule,
        judged.path,
        judged
            .position
            .map_or_else(|| "?".to_owned(), |one| one.line.to_string())
    );
    let discharged = route.discharged();
    if route.reaching().is_empty() && !discharged.is_empty() {
        let mut proofs: Vec<&str> = discharged.iter().map(|one| one.proof.name()).collect();
        proofs.sort_unstable();
        proofs.dedup();
        return format!(
            "no test could have noticed {at}: every one of the {} targets that reach it was \
             removed without being run, by {}",
            discharged.len(),
            proofs.join(" and ")
        );
    }
    let reaching = route.reaching().len();
    format!(
        "no test noticed {at}; {reaching} {} ran it and none of them noticed",
        if reaching == 1 { "target" } else { "targets" }
    )
}

fn finding_of(judged: &Judged) -> Option<Finding> {
    let (kind, detail) = match &judged.disposition {
        Disposition::Survived { route } => (FindingKind::SurvivingMutant, survived(judged, route)),
        Disposition::Unreached => (
            FindingKind::SurvivingMutant,
            format!(
                "no measured target reaches {} at {}: each of them was measured, was asked, \
                 and answered that nothing of it executes the position",
                judged.rule, judged.path
            ),
        ),
        Disposition::Unconfirmed { on, why } => {
            (FindingKind::FailingTest, format!("{on}: {}", why.detail()))
        }
        Disposition::Errored { on, detail } => (
            FindingKind::TargetMissing,
            format!("{on}: the mutation could not be measured: {detail}"),
        ),
        Disposition::Waited { on } => (
            FindingKind::WaitedMutant,
            format!(
                "this machine stopped waiting for {on} with {} at {} active: an expired bound \
                 establishes nothing about the mutation. With a step allowance, the bound ends \
                 only a process that raised no step for a whole window, which is a wait rather \
                 than a loop; a mutation that spins is stopped by the count instead",
                judged.rule, judged.path
            ),
        ),
        Disposition::StepLimitReached { on, boundary } => (
            FindingKind::StepLimitReachedMutant,
            format!(
                "{on} reached step {} with {} at {} active, one beyond its configured \
                 allowance of {}. No matched control established that the mutation caused \
                 divergence, so this is not a detection",
                boundary.observed(),
                judged.rule,
                judged.path,
                boundary.limit()
            ),
        ),
        Disposition::Killed { .. }
        | Disposition::Rejected { .. }
        | Disposition::Equivalent { .. } => {
            return None;
        }
    };
    let mut finding = Finding::new(kind, &judged.display_id, &detail);
    finding.path = Some(judged.path.clone());
    finding.position = judged.position;
    Some(finding)
}

/// What is being measured: the prepared engine session and what the baseline saw.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// The prepared session.
    pub session: &'a Session,
    /// Every target the baseline measured.
    pub baseline: &'a [Measured],
    /// What the session's catalog puts to the tests, which decides what the recording calls each execution.
    pub perturbing: Perturbing,
}

/// What a catalog puts at each of its sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perturbing {
    /// A change to the program, whose executions, routes, probes and controls every reader of mutations reads.
    Mutants,
    /// A failed call, whose executions are recorded as fault executions and nothing else, so no reader of mutations ever counts one (ADR 0032).
    Faults,
}

impl Perturbing {
    /// Whether a phase putting this judges a catalog entry that perturbs `what`: a faulted session also holds the mutations a fault is put beside, which it asks about only there.
    #[must_use]
    pub const fn judges(self, what: rust_mutants::rule::Perturbs) -> bool {
        match (self, what) {
            (Self::Mutants, rust_mutants::rule::Perturbs::Program)
            | (Self::Faults, rust_mutants::rule::Perturbs::Environment) => true,
            (Self::Mutants, rust_mutants::rule::Perturbs::Environment)
            | (Self::Faults, rust_mutants::rule::Perturbs::Program)
            | (Self::Mutants | Self::Faults, rust_mutants::rule::Perturbs::Crash) => false,
        }
    }
}

/// What to measure and how.
#[derive(Debug, Clone)]
pub struct MutationOptions {
    /// Arguments for the test binaries.
    pub test_args: Vec<String>,
    /// What earlier runs established about individual mutants, and what this run knows of the targets they name.
    /// `None` for a run that establishes everything itself.
    pub evidence: Option<Evidence>,
    /// How many mutations to measure at once.
    pub jobs: rust_mutants::run::Jobs,
    /// Whether a resource only one test may hold at a time forces the run to measure one mutation at a time.
    pub exclusive: bool,
    /// Which part of the catalog to judge.
    /// `None` judges every one of them.
    pub shard: Option<rust_mutants::run::Shard>,
}

/// Where a run reads and writes what is established about individual mutants.
#[derive(Debug, Clone)]
pub struct Evidence {
    /// The directory records live in.
    pub root: PathBuf,
    /// This run, which is what a record it writes names.
    pub run_id: String,
    /// The behaviour key of every target this run's baseline saw pass on the original tree, by target identity.
    pub standing: store::Standing,
    /// What a person calls each of those targets, by identity.
    /// A record names identities, because a name is what a reader reads and an identity is what a route decides.
    pub names: BTreeMap<String, String>,
}

impl Evidence {
    /// The identity of the target a person calls `name`.
    #[must_use]
    pub fn identity(&self, name: &str) -> Option<&str> {
        self.names
            .iter()
            .find(|(_id, called)| called.as_str() == name)
            .map(|(id, _called)| id.as_str())
    }

    /// The identities of the targets a route names, or which of them is not a target this run's baseline saw pass.
    ///
    /// # Errors
    /// Names the first target it could not resolve.
    pub fn identities(&self, names: &[&str]) -> Result<Vec<String>, store::Refusal> {
        names
            .iter()
            .map(|name| {
                self.identity(name).map(ToOwned::to_owned).ok_or_else(|| {
                    store::Refusal::TargetUnknown {
                        target: (*name).to_owned(),
                    }
                })
            })
            .collect()
    }
}

/// What an interrupted run already judged, and where to record what this one judges.
pub struct Resume<'a> {
    /// The state that run left, or nothing for a run starting cold.
    pub state: Option<&'a crate::checkpoint::State>,
    /// Called with each mutant as it finishes, so the caller can save what has been established before it can be lost.
    pub record: &'a mut dyn FnMut(&Judged) -> Result<(), crate::error::RunnerError>,
}

impl std::fmt::Debug for Resume<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resume")
            .field("has_state", &self.state.is_some())
            .finish_non_exhaustive()
    }
}

/// How many places of the catalog each reason left unmutated.
fn skip_census(session: &Session) -> Result<BTreeMap<String, u64>, crate::error::RunnerError> {
    let mut census = BTreeMap::new();
    for skip in session.skips() {
        let count = census.entry(skip.reason.name().to_owned()).or_insert(0_u64);
        *count = count
            .checked_add(1)
            .ok_or(crate::report::CountError::Overflow {
                field: "mutation skip census",
            })?;
    }
    Ok(census)
}

/// Runs every accepted mutant against the tests that could notice it, continuing from what an interrupted run had already judged.
///
/// # Errors
/// The engine's refusals.
/// A mutant that survives is not an error: it is the finding.
pub fn run_resuming(
    subject: Subject<'_>,
    options: &MutationOptions,
    resume: &mut Resume<'_>,
    mut reporting: crate::assure::baseline::Reporting<'_, '_>,
) -> Result<Mutation, crate::error::RunnerError> {
    let watch = reporting.watch;
    let (session, baseline) = (subject.session, subject.baseline);
    let phase = watch.trace.phase("mutation-judge");
    let mut mutation = Mutation {
        skips: skip_census(session)?,
        ..Mutation::default()
    };

    if subject.perturbing == Perturbing::Mutants {
        record_probe(watch, session, baseline)?;
    }

    let catalog = session.catalog();
    let rejected: BTreeMap<&str, &str> = session
        .rejections()
        .iter()
        .map(|rejection| (rejection.id.as_str(), rejection.diagnostic.as_str()))
        .collect();
    let mutants: Vec<&Mutant> = catalog
        .mutants()
        .iter()
        .filter(|mutant| options.shard.is_none_or(|shard| shard.holds(mutant.index)))
        .filter(|mutant| {
            subject
                .perturbing
                .judges(mutant.candidate.rule.family.perturbs())
        })
        .collect();
    let total = u64::try_from(mutants.len())
        .map_err(|error| crate::targets::TargetError::invalid("mutation target count", error))?;
    let controls = Controls::default();
    let judging = Judging {
        subject,
        options,
        controls: &controls,
        watch,
        quiet: schedule::Quiet::default(),
    };

    let available = schedule::available()?;
    let worker_count = schedule::workers(options.jobs, available, options.exclusive);
    let measured = schedule::measure(&mutants, worker_count, |_at, mutant| {
        establish(mutant, &judging, resume.state, &rejected)
    })?;

    for (index, answer) in measured.into_iter().enumerate() {
        let judged = answer?;
        let done = u64::try_from(index)
            .map_err(|error| crate::targets::TargetError::invalid("mutation progress", error))?
            .checked_add(1)
            .ok_or_else(|| {
                crate::targets::TargetError::invalid(
                    "mutation progress",
                    "the mutation progress count overflowed",
                )
            })?;
        reporting.about(
            crate::assure::baseline::Step::about(&watching(&judged), &judged.display_id),
            done,
            total,
        )?;
        (resume.record)(&judged)?;
        mutation.judged.push(judged);
    }
    mutation.sources = session.catalog().sources().map_err(|refused| {
        rust_mutants::EngineError::from(rust_mutants::discover::DiscoverError::from(refused))
    })?;
    let confirmed: Vec<Drift> = mutation
        .judged
        .iter()
        .flat_map(|judged| judged.observed.iter().cloned())
        .collect();
    let compared = compared_alone(session, &confirmed, watch)?;
    mutation.drift = crate::report::drift::folded(
        session.touched().targets.keys().map(String::as_str),
        confirmed.into_iter().chain(compared),
    );
    repaired(&judging, &mut mutation)?;
    phase.end();
    Ok(mutation)
}

/// Every disposition that rests on a target whose reach moved, run again against that target with its reach recorded, and replaced by what that run decides where it reached the site (ADR 0036).
///
/// # Errors
/// The engine's refusals, and an interruption.
fn repaired(
    judging: &Judging<'_>,
    mutation: &mut Mutation,
) -> Result<(), crate::error::RunnerError> {
    let session = judging.subject.session;
    let moved: Vec<String> = mutation
        .drift
        .iter()
        .filter_map(|one| match one {
            Drift::Moved { target, .. } => Some(target.clone()),
            Drift::Held { .. } | Drift::NotMeasured { .. } => None,
        })
        .collect();
    for target in &moved {
        let Some(measured) = judging
            .subject
            .baseline
            .iter()
            .find(|measured| measured.target.name() == *target)
        else {
            continue;
        };
        for judged in &mut mutation.judged {
            let resting = matches!(
                judged.disposition,
                Disposition::Survived { .. } | Disposition::Unreached
            ) && crate::report::drift::rests_on(judged.routing.as_ref(), target);
            if !resting {
                continue;
            }
            let Some(mutant) = session
                .catalog()
                .mutants()
                .iter()
                .find(|mutant| mutant.index == judged.catalog_index)
            else {
                continue;
            };
            if judging.watch.cancel.is_cancelled() {
                return Err(crate::error::RunnerError::Interrupted);
            }
            if !repair(judging, (judged, mutant), target, measured)? {
                continue;
            }
            let count = mutation.repaired.entry(target.clone()).or_insert(0);
            *count = count
                .checked_add(1)
                .ok_or(crate::report::CountError::Overflow {
                    field: "repaired dispositions",
                })?;
        }
    }
    Ok(())
}

/// Runs, once and whole, every target no control confirming a kill compared, so that every target's reach is compared with a second run of it whether or not it noticed anything.
fn compared_alone(
    session: &Session,
    confirmed: &[Drift],
    watch: Watch<'_>,
) -> Result<Vec<Drift>, crate::error::RunnerError> {
    let seen: BTreeSet<&str> = confirmed.iter().map(Drift::target).collect();
    let mut compared = Vec::new();
    for target in session.touched().targets.keys() {
        if seen.contains(target.as_str()) || watch.cancel.is_cancelled() {
            continue;
        }
        let request = Request::new(String::new()).with_target(target.as_str());
        let control = session.control(&request, watch.cancel, Observing::Reach)?;
        for one in &control.observed {
            let drift = Drift::of(&one.target, &one.steadiness);
            watch.trace.drift(crate::trace::DriftRecord {
                mutant: None,
                observed: drift.clone(),
            });
            compared.push(drift);
        }
    }
    Ok(compared)
}

/// What one mutant comes to, without committing anything a report will carry.
fn establish(
    mutant: &Mutant,
    judging: &Judging<'_>,
    state: Option<&crate::checkpoint::State>,
    rejected: &BTreeMap<&str, &str>,
) -> Result<Judged, crate::error::RunnerError> {
    let (session, options, watch) = (judging.subject.session, judging.options, judging.watch);
    let position = session.position(mutant).map(|at| crate::report::Position {
        line: at.line,
        column: at.byte_column,
        character_column: at.char_column,
    });
    let mut source: Option<String> = None;
    let mut routing: Option<crate::report::Routing> = None;
    let disposition = if let Some(saved) = state.and_then(|state| state.mutant(mutant.id.as_str()))
    {
        let crate::checkpoint::SavedDisposition::Killed { by, before } = &saved.disposition;
        routing = Some(crate::report::Routing::of(
            &session.route(mutant),
            through(before.iter().cloned(), by),
        ));
        inherited(saved)
    } else if let Some(diagnostic) = rejected.get(mutant.id.as_str()) {
        if judging.subject.perturbing == Perturbing::Faults {
            record_rejection(watch, mutant, diagnostic);
        }
        Disposition::Rejected {
            diagnostic: (*diagnostic).to_owned(),
        }
    } else {
        let route = session.route(mutant);
        let consulted = reuse(options, &route, mutant.id.as_str());
        match judging.subject.perturbing {
            Perturbing::Mutants => record_route(watch, mutant, &route, &consulted),
            Perturbing::Faults => record_fault_route(watch, mutant, &route),
        }
        if let Consulted::Believed {
            disposition,
            answered,
            run_id,
        } = consulted
        {
            source = Some(run_id);
            routing = Some(crate::report::Routing::of(&route, answered));
            disposition
        } else {
            let (established, asked) = judge(judging, mutant, route.clone())?;
            match keep(options, mutant.id.as_str(), (&route, &asked), &established)? {
                Kept::Written => {}
                Kept::NotKept(_costs_the_next_run_its_time_and_this_one_no_verdict) => {}
            }
            routing = Some(crate::report::Routing::of(&route, asked));
            established
        }
    };
    let original = std::str::from_utf8(&mutant.candidate.original)
        .map_err(|source| MutationTextError::Original { source })?;
    let replacement = std::str::from_utf8(&mutant.candidate.replacement)
        .map_err(|source| MutationTextError::Replacement { source })?;
    Ok(Judged {
        catalog_index: mutant.index,
        id: mutant.id.to_string(),
        display_id: mutant.display_id.to_string(),
        path: mutant.candidate.path.clone(),
        rule: mutant.candidate.rule.name.to_owned(),
        item: judging
            .subject
            .session
            .item_of(mutant.index)
            .unwrap_or_default()
            .to_owned(),
        original: original.to_owned(),
        replacement: replacement.to_owned(),
        position,
        disposition,
        routing,
        source_run_id: source,
        observed: judging.controls.taken(mutant.id.as_str())?,
    })
}

/// Records what the infection layer measured for each target the baseline ran.
fn record_probe(
    watch: Watch<'_>,
    session: &Session,
    baseline: &[Measured],
) -> Result<(), crate::assure::run::RunInvariantError> {
    let touched = &session.verified().touched;
    if touched.narrowing.compared.is_empty() {
        return Ok(());
    }
    for measured in baseline {
        let seen = touched.targets.get(&measured.target.name());
        let infected = seen
            .map(|one| {
                let named: BTreeSet<&u32> = one
                    .infected
                    .tests
                    .values()
                    .flat_map(|indices| indices.iter())
                    .chain(one.infected.loose.iter())
                    .collect();
                u64::try_from(named.len()).map_err(|_outside_wire_range| {
                    crate::assure::run::RunInvariantError::ProbeCountOutsideWire {
                        count: named.len(),
                    }
                })
            })
            .transpose()?;
        watch.trace.probe_exec(crate::trace::ProbeExecRecord {
            target: measured.target.id.to_string(),
            outcome: if seen.is_some() {
                "measured".to_owned()
            } else {
                "not-measured".to_owned()
            },
            infected,
        });
    }
    Ok(())
}

/// Records how one mutant's tests were chosen, and whether this run established the answer itself.
/// Records a fault the compiler refused, which is what `not-put` rests on.
fn record_rejection(watch: Watch<'_>, mutant: &Mutant, diagnostic: &str) {
    watch
        .trace
        .fault_rejected(crate::trace::FaultRejectedRecord {
            fault: mutant.display_id.to_string(),
            diagnostic: crate::assure::run::first_line(diagnostic),
        });
}

/// Records which targets reach a fault, which is what `unreached` rests on, apart from every mutant route.
fn record_fault_route(watch: Watch<'_>, mutant: &Mutant, route: &Route) {
    watch.trace.fault_route(crate::trace::FaultRouteRecord {
        fault: mutant.display_id.to_string(),
        reaching: route
            .reaching()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
    });
}

fn record_route(watch: Watch<'_>, mutant: &Mutant, route: &Route, consulted: &Consulted) {
    watch.trace.route(crate::trace::RouteRecord {
        mutant: mutant.display_id.to_string(),
        granularity: route.granularity(),
        fallback: route.fallback(),
        reaching: route
            .reaching()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        tests: route
            .tests()
            .into_iter()
            .map(|(target, tests)| crate::trace::AskedRecord { target, tests })
            .collect(),
        discharged: route
            .discharged()
            .iter()
            .map(|one| crate::trace::DischargeRecord {
                target: one.target.clone(),
                proof: one.proof.name().to_owned(),
            })
            .collect(),
        considered: route.considered().to_vec(),
        reused: match consulted {
            Consulted::Believed { run_id, .. } => Some(run_id.clone()),
            Consulted::NotKept | Consulted::Refused(_) => None,
        },
        refused: match consulted {
            Consulted::Refused(refusal) => Some(refusal.name().to_owned()),
            Consulted::NotKept | Consulted::Believed { .. } => None,
        },
    });
}

/// What a store of earlier answers had to say about one mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consulted {
    /// This run keeps no store of what earlier runs established, so there was nothing to ask.
    NotKept,
    /// What an earlier run established, and the run that established it.
    Believed {
        /// What that run established about the mutant.
        disposition: Disposition,
        /// The targets that run asked, in order, with what each answered.
        answered: Vec<crate::report::Answered>,
        /// The run that established it.
        run_id: String,
    },
    /// A store was asked and this run may not believe what it holds.
    Refused(store::Refusal),
}

/// What an earlier run established about this mutant, or why this run may not believe it.
#[must_use]
pub fn reuse(options: &MutationOptions, route: &Route, mutant: &str) -> Consulted {
    let Some(evidence) = options.evidence.as_ref() else {
        return Consulted::NotKept;
    };
    let mutant = match rust_mutants::id::HexDigest::try_from(mutant) {
        Ok(mutant) => mutant,
        Err(error) => {
            return Consulted::Refused(store::Refusal::Unreadable {
                message: error.to_string(),
            });
        }
    };
    let record = match store::read(&evidence.root, &mutant) {
        Ok(Some(record)) => record,
        Ok(None) => return Consulted::Refused(store::Refusal::Nothing),
        Err(error) => {
            return Consulted::Refused(store::Refusal::Unreadable {
                message: error.to_string(),
            });
        }
    };
    let asking = match asking(route, evidence) {
        Ok(named) => named,
        Err(refusal) => return Consulted::Refused(refusal),
    };
    if let Err(refusal) = record.believable(&asking, &evidence.standing) {
        return Consulted::Refused(refusal);
    }
    let named = |target: &String| {
        evidence
            .names
            .get(target)
            .cloned()
            .unwrap_or_else(|| target.clone())
    };
    let (disposition, answered) = match &record.outcome {
        store::Outcome::Killed { target, before, .. } => (
            Disposition::Killed { by: named(target) },
            through(
                before.iter().map(|answer| crate::report::Answered {
                    target: named(&answer.target),
                    outcome: answer.outcome,
                }),
                &named(target),
            ),
        ),
        store::Outcome::Survived { .. } => (
            Disposition::Survived {
                route: route.clone(),
            },
            asking
                .iter()
                .map(|target| crate::report::Answered {
                    target: named(target),
                    outcome: Recorded::Survived,
                })
                .collect(),
        ),
    };
    Consulted::Believed {
        disposition,
        answered,
        run_id: record.run_id,
    }
}

/// The answers a run gave up to and including the kill by `by`, from the ones it gave `before`.
fn through(
    before: impl Iterator<Item = crate::report::Answered>,
    by: &str,
) -> Vec<crate::report::Answered> {
    before
        .chain(std::iter::once(crate::report::Answered {
            target: by.to_owned(),
            outcome: Recorded::Killed,
        }))
        .collect()
}

/// The identities of the targets a route names, in the order a run asks them.
///
/// # Errors
/// Names the first target this run's baseline has no identity for.
fn asking(route: &Route, evidence: &Evidence) -> Result<Vec<String>, store::Refusal> {
    let names: BTreeSet<&str> = route.reaching().into_iter().collect();
    evidence.identities(&names.into_iter().collect::<Vec<_>>())
}

/// What became of one run's attempt to record what it established for the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// The record is on disk.
    Written,
    /// Nothing was written, for a reason this run can name.
    NotKept(NotKept),
}

/// Why a run that established something recorded nothing for the next one.
///
/// The mirror of [`store::Refusal`], which says why a record already on disk is not believed.
/// Both are fail-safe: an answer nobody can reuse costs the next run its time and costs this one's verdict nothing.
/// What it may not do is look the same as having had nothing to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotKept {
    /// This run keeps no evidence directory.
    NoStore,
    /// The target the kill names is not one this run measured a baseline for.
    TargetUnknown {
        /// The target with no identity.
        target: String,
    },
    /// This run's own baseline did not see a target pass on the original tree, so it can vouch for nothing about it.
    NotPassing {
        /// The target this run cannot vouch for.
        target: String,
    },
    /// The route names a target this run's baseline has no identity for.
    RouteUnreadable {
        /// What reading the route said.
        refusal: store::Refusal,
    },
    /// A survival that reached no target at all, which says nothing a later run could check.
    NothingReached,
    /// The disposition is not one a later run may inherit.
    NotAVerdict {
        /// What this run concluded.
        disposition: &'static str,
    },
}

/// Records what this run established for a later run whose targets retain the same behaviour keys.
///
/// Only a named kill and a complete survival are cached.
/// A step boundary is explicitly a non-verdict and is never reusable.
///
/// # Errors
/// Returns [`store::StoreError`] when the identity or durable cache record cannot be represented or written exactly.
pub fn keep(
    options: &MutationOptions,
    mutant: &str,
    (route, asked): (&Route, &[crate::report::Answered]),
    disposition: &Disposition,
) -> Result<Kept, store::StoreError> {
    let Some(evidence) = options.evidence.as_ref() else {
        return Ok(Kept::NotKept(NotKept::NoStore));
    };
    let mutant = rust_mutants::id::HexDigest::try_from(mutant).map_err(|error| {
        store::StoreError::Corrupt {
            path: evidence.root.clone(),
            message: error.to_string(),
        }
    })?;
    let outcome = match disposition {
        Disposition::Killed { by } => {
            let Some(target) = evidence.identity(by) else {
                return Ok(Kept::NotKept(NotKept::TargetUnknown { target: by.clone() }));
            };
            let Some(key) = evidence.standing.passing.get(target) else {
                return Ok(Kept::NotKept(NotKept::NotPassing {
                    target: target.to_owned(),
                }));
            };
            let mut before = Vec::new();
            for answer in asked.iter().take_while(|answer| answer.target != *by) {
                let Some(identity) = evidence.identity(&answer.target) else {
                    return Ok(Kept::NotKept(NotKept::TargetUnknown {
                        target: answer.target.clone(),
                    }));
                };
                let Some(key) = evidence.standing.passing.get(identity) else {
                    return Ok(Kept::NotKept(NotKept::NotPassing {
                        target: identity.to_owned(),
                    }));
                };
                before.push(store::Answer {
                    target: identity.to_owned(),
                    key: key.clone(),
                    outcome: answer.outcome,
                });
            }
            store::Outcome::Killed {
                target: target.to_owned(),
                key: key.clone(),
                before,
            }
        }
        Disposition::Survived { .. } => {
            let mut targets = BTreeMap::new();
            let named = match answered(route, evidence) {
                Ok(named) => named,
                Err(refusal) => {
                    return Ok(Kept::NotKept(NotKept::RouteUnreadable { refusal }));
                }
            };
            for target in named {
                let Some(key) = evidence.standing.passing.get(&target) else {
                    return Ok(Kept::NotKept(NotKept::NotPassing { target }));
                };
                targets.insert(target, key.clone());
            }
            if targets.is_empty() {
                return Ok(Kept::NotKept(NotKept::NothingReached));
            }
            store::Outcome::Survived { targets }
        }
        Disposition::StepLimitReached { .. }
        | Disposition::Waited { .. }
        | Disposition::Rejected { .. }
        | Disposition::Unreached
        | Disposition::Equivalent { .. }
        | Disposition::Unconfirmed { .. }
        | Disposition::Errored { .. } => {
            return Ok(Kept::NotKept(NotKept::NotAVerdict {
                disposition: disposition.name(),
            }));
        }
    };
    store::write(
        &evidence.root,
        &store::record(mutant, &evidence.run_id, outcome),
    )?;
    Ok(Kept::Written)
}

/// The targets a route's answer is about: the ones it named, or every target this run saw pass when the package suite is what answered.
///
/// # Errors
/// Names the first target this run's baseline has no identity for.
pub fn answered(route: &Route, evidence: &Evidence) -> Result<Vec<String>, store::Refusal> {
    evidence.identities(&route.reaching())
}

/// The disposition a checkpoint's record stands for, or nothing when this release does not inherit it.
#[must_use]
pub fn inherited(saved: &crate::checkpoint::SavedMutant) -> Disposition {
    match &saved.disposition {
        crate::checkpoint::SavedDisposition::Killed { by, .. } => {
            Disposition::Killed { by: by.clone() }
        }
    }
}

/// Runs one mutant against the tests its route named, stopping at the first confirmed catch.
struct Judging<'a> {
    subject: Subject<'a>,
    options: &'a MutationOptions,
    controls: &'a Controls,
    watch: Watch<'a>,
    quiet: schedule::Quiet,
}

/// The finite lattice used to combine target observations.
///
/// Declaration order is the conservative strength order.
/// Deriving [`Ord`] makes the compiler generate the total order from this single closed list;
/// [`std::cmp::max`] then supplies the idempotent, commutative and associative join without a second handwritten rank ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TargetObservation {
    Survived,
    StepLimitReached,
    Waited,
    Unconfirmed,
    Errored,
}

impl TargetObservation {
    fn join(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

/// The fact one target supplied before cross-target aggregation.
enum TargetFact {
    Survived,
    Killed {
        on: String,
        retry: Request,
    },
    Waited {
        on: String,
        retry: Request,
    },
    StepLimitReached {
        on: String,
        boundary: crate::report::StepBoundary,
    },
    Errored {
        on: String,
        detail: String,
    },
}

impl TargetFact {
    const fn outcome(&self) -> Recorded {
        match self {
            Self::Survived => Recorded::Survived,
            Self::Killed { .. } => Recorded::Killed,
            Self::Waited { .. } => Recorded::Waited,
            Self::StepLimitReached { .. } => Recorded::StepLimitReached,
            Self::Errored { .. } => Recorded::Errored,
        }
    }
}

/// A non-answer retained until every target has had a chance to supply a kill.
enum Unsettled {
    StepLimitReached {
        on: String,
        boundary: crate::report::StepBoundary,
    },
    Waited {
        on: String,
        retry: Request,
        answered: AnsweredIndex,
    },
    Unconfirmed {
        on: String,
        why: Unconfirmed,
    },
    Errored {
        on: String,
        detail: String,
    },
}

impl Unsettled {
    const fn observation(&self) -> TargetObservation {
        match self {
            Self::StepLimitReached { .. } => TargetObservation::StepLimitReached,
            Self::Waited { .. } => TargetObservation::Waited,
            Self::Unconfirmed { .. } => TargetObservation::Unconfirmed,
            Self::Errored { .. } => TargetObservation::Errored,
        }
    }

    fn on(&self) -> &str {
        match self {
            Self::StepLimitReached { on, .. }
            | Self::Waited { on, .. }
            | Self::Unconfirmed { on, .. }
            | Self::Errored { on, .. } => on,
        }
    }

    fn disposition(self) -> Disposition {
        match self {
            Self::StepLimitReached { on, boundary } => {
                Disposition::StepLimitReached { on, boundary }
            }
            Self::Waited { on, .. } => Disposition::Waited { on },
            Self::Unconfirmed { on, why } => Disposition::Unconfirmed { on, why },
            Self::Errored { on, detail } => Disposition::Errored { on, detail },
        }
    }
}

fn select_unsettled(facts: Vec<Unsettled>, observation: TargetObservation) -> Option<Unsettled> {
    facts
        .into_iter()
        .filter(|fact| fact.observation() == observation)
        .min_by(|left, right| left.on().cmp(right.on()))
}

/// A checked handle to a row in the answer ledger.
#[derive(Clone, Copy)]
struct AnsweredIndex(usize);

impl AnsweredIndex {
    fn append(answered: &mut Vec<crate::report::Answered>, row: crate::report::Answered) -> Self {
        let index = Self(answered.len());
        answered.push(row);
        index
    }

    fn mark_unconfirmed(
        self,
        answered: &mut [crate::report::Answered],
        target: &str,
    ) -> Result<(), crate::assure::run::RunInvariantError> {
        let row = answered.get_mut(self.0).ok_or_else(|| {
            crate::assure::run::RunInvariantError::MissingMutationAnswer {
                target: target.to_owned(),
            }
        })?;
        row.outcome = Recorded::Unconfirmed;
        Ok(())
    }
}

/// The checked state accumulated while target answers are combined.
struct Aggregation {
    answered: Vec<crate::report::Answered>,
    unsettled: Vec<Unsettled>,
    observation: TargetObservation,
}

impl Aggregation {
    const fn new() -> Self {
        Self {
            answered: Vec::new(),
            unsettled: Vec::new(),
            observation: TargetObservation::Survived,
        }
    }

    fn missing(&mut self, target: &str) {
        self.answered.push(crate::report::Answered {
            target: target.to_owned(),
            outcome: Recorded::Errored,
        });
        self.unsettled.push(Unsettled::Errored {
            on: target.to_owned(),
            detail: "the route named a target absent from the measured baseline".to_owned(),
        });
        self.observation = self.observation.join(TargetObservation::Errored);
    }

    fn observe(
        &mut self,
        judging: &Judging<'_>,
        (mutant, target): (&Mutant, &str),
        fact: TargetFact,
    ) -> Result<Option<Disposition>, crate::error::RunnerError> {
        let answered = AnsweredIndex::append(
            &mut self.answered,
            crate::report::Answered {
                target: target.to_owned(),
                outcome: fact.outcome(),
            },
        );
        match fact {
            TargetFact::Survived => {
                self.observation = self.observation.join(TargetObservation::Survived);
            }
            TargetFact::Killed { on, retry } => {
                match confirm(judging, (mutant, &on), &retry, ExpectedReproduction::Killed)? {
                    Ok(()) => return Ok(Some(Disposition::Killed { by: on })),
                    Err(why) => {
                        answered.mark_unconfirmed(&mut self.answered, &on)?;
                        self.unsettled.push(Unsettled::Unconfirmed { on, why });
                        self.observation = self.observation.join(TargetObservation::Unconfirmed);
                    }
                }
            }
            TargetFact::Waited { on, retry } => {
                self.unsettled.push(Unsettled::Waited {
                    on,
                    retry,
                    answered,
                });
                self.observation = self.observation.join(TargetObservation::Waited);
            }
            TargetFact::StepLimitReached { on, boundary } => {
                self.unsettled
                    .push(Unsettled::StepLimitReached { on, boundary });
                self.observation = self.observation.join(TargetObservation::StepLimitReached);
            }
            TargetFact::Errored { on, detail } => {
                self.unsettled.push(Unsettled::Errored { on, detail });
                self.observation = self.observation.join(TargetObservation::Errored);
            }
        }
        Ok(None)
    }

    fn finish(
        mut self,
        judging: &Judging<'_>,
        mutant: &Mutant,
        route: Route,
    ) -> Result<(Disposition, Vec<crate::report::Answered>), crate::error::RunnerError> {
        let Some(selected) = select_unsettled(self.unsettled, self.observation) else {
            let disposition = match self.observation {
                TargetObservation::Survived => Disposition::Survived { route },
                TargetObservation::StepLimitReached
                | TargetObservation::Waited
                | TargetObservation::Unconfirmed
                | TargetObservation::Errored => Disposition::Errored {
                    on: SUITE.to_owned(),
                    detail: "target observations could not be reduced to their payload".to_owned(),
                },
            };
            return Ok((disposition, self.answered));
        };
        if let Unsettled::Waited {
            on,
            retry,
            answered,
        } = selected
        {
            let disposition =
                match confirm(judging, (mutant, &on), &retry, ExpectedReproduction::Waited)? {
                    Ok(()) => Disposition::Waited { on },
                    Err(why) => {
                        answered.mark_unconfirmed(&mut self.answered, &on)?;
                        Disposition::Unconfirmed { on, why }
                    }
                };
            return Ok((disposition, self.answered));
        }
        Ok((selected.disposition(), self.answered))
    }
}

fn judge(
    judging: &Judging<'_>,
    mutant: &Mutant,
    route: Route,
) -> Result<(Disposition, Vec<crate::report::Answered>), crate::error::RunnerError> {
    let baseline = judging.subject.baseline;
    if let Route::Discharged { .. } = route {
        return Ok((Disposition::Survived { route }, Vec::new()));
    }
    if route.reaching().is_empty() {
        return Ok((Disposition::Unreached, Vec::new()));
    }
    let mut aggregation = Aggregation::new();
    let targets: BTreeSet<&str> = route.reaching().into_iter().collect();
    for target in targets {
        let Some(measured) = baseline
            .iter()
            .find(|measured| measured.target.name() == target)
        else {
            aggregation.missing(target);
            continue;
        };
        let established = against(judging, mutant, Some(measured))?;
        if judging.watch.cancel.is_cancelled() {
            return Err(crate::error::RunnerError::Interrupted);
        }
        if let Some(disposition) = aggregation.observe(judging, (mutant, target), established)? {
            return Ok((disposition, aggregation.answered));
        }
    }
    aggregation.finish(judging, mutant, route)
}

/// What one mutation comes to against one test before the route aggregates every target.
fn against(
    judging: &Judging<'_>,
    mutant: &Mutant,
    measured: Option<&Measured>,
) -> Result<TargetFact, crate::error::RunnerError> {
    let (session, options, watch) = (judging.subject.session, judging.options, judging.watch);
    let request = request_for(mutant.id.as_str(), measured, &options.test_args);
    let mut result = judging
        .quiet
        .shared(|| session.exec(&request, watch.cancel))??;
    record_exec(
        watch,
        &Ran {
            mutant,
            measured,
            request: &request,
            result: &result,
            alone: false,
            perturbing: judging.subject.perturbing,
        },
    )?;
    if quiet_measurement_due(result.outcome(), watch.cancel.is_cancelled()) {
        result = judging
            .quiet
            .alone(|| session.exec(&request, watch.cancel))??;
        record_exec(
            watch,
            &Ran {
                mutant,
                measured,
                request: &request,
                result: &result,
                alone: true,
                perturbing: judging.subject.perturbing,
            },
        )?;
    }
    Ok(fact_of(request, measured, &result))
}

/// Runs `judged` again against the moved `target`, records what it came to, and replaces its disposition where the run decided one; whether it did.
fn repair(
    judging: &Judging<'_>,
    (judged, mutant): (&mut Judged, &Mutant),
    target: &str,
    measured: &Measured,
) -> Result<bool, crate::error::RunnerError> {
    let was = judged.disposition.name();
    let (fact, reach) = against_reaching(judging, mutant, measured)?;
    let mut aggregation = Aggregation::new();
    let (now, answered) = match aggregation.observe(judging, (mutant, target), fact)? {
        Some(decided) => (decided, aggregation.answered),
        None => aggregation.finish(judging, mutant, judging.subject.session.route(mutant))?,
    };
    let reached = match reach {
        rust_mutants::session::SiteReach::Reached => crate::trace::SiteReached::Reached,
        rust_mutants::session::SiteReach::NotReached => crate::trace::SiteReached::NotReached,
        rust_mutants::session::SiteReach::Unrecorded => crate::trace::SiteReached::Unrecorded,
    };
    let replaced = reached == crate::trace::SiteReached::Reached
        || !matches!(now, Disposition::Survived { .. });
    judging.watch.trace.repair(crate::trace::RepairRecord {
        mutant: judged.display_id.clone(),
        target: target.to_owned(),
        was: was.to_owned(),
        now: if replaced { now.name() } else { was }.to_owned(),
        reached,
    });
    if replaced {
        judged.disposition = now;
        if let Some(routing) = judged.routing.as_mut() {
            routing.reaching.push(target.to_owned());
            routing.answered.extend(answered);
        }
    }
    Ok(replaced)
}

/// What one mutation comes to against one target whose reach moved, run with its guards recording, and whether that run reached the mutation's site (ADR 0036).
fn against_reaching(
    judging: &Judging<'_>,
    mutant: &Mutant,
    measured: &Measured,
) -> Result<(TargetFact, rust_mutants::session::SiteReach), crate::error::RunnerError> {
    let (session, options, watch) = (judging.subject.session, judging.options, judging.watch);
    let request = request_for(mutant.id.as_str(), Some(measured), &options.test_args);
    let mut alone = false;
    let (mut result, mut reach) = judging
        .quiet
        .shared(|| session.exec_reaching(&request, watch.cancel))??;
    record_exec(
        watch,
        &Ran {
            mutant,
            measured: Some(measured),
            request: &request,
            result: &result,
            alone,
            perturbing: judging.subject.perturbing,
        },
    )?;
    if quiet_measurement_due(result.outcome(), watch.cancel.is_cancelled()) {
        alone = true;
        (result, reach) = judging
            .quiet
            .alone(|| session.exec_reaching(&request, watch.cancel))??;
        record_exec(
            watch,
            &Ran {
                mutant,
                measured: Some(measured),
                request: &request,
                result: &result,
                alone,
                perturbing: judging.subject.perturbing,
            },
        )?;
    }
    Ok((fact_of(request, Some(measured), &result), reach))
}

/// What one execution of a mutation against one target says, before the route aggregates every target.
fn fact_of(request: Request, measured: Option<&Measured>, result: &MutantResult) -> TargetFact {
    let name = measured.map_or_else(
        || {
            if result.target.is_empty() {
                SUITE.to_owned()
            } else {
                result.target.clone()
            }
        },
        |one| one.target.name(),
    );
    match &result.conclusion {
        MutantConclusion::Survived => TargetFact::Survived,
        MutantConclusion::Killed => TargetFact::Killed {
            on: name,
            retry: narrowed(request, measured, &result.target),
        },
        MutantConclusion::Waited => TargetFact::Waited {
            on: name,
            retry: narrowed(request, measured, &result.target),
        },
        MutantConclusion::StepLimitReached { notice } => {
            let Some(boundary) =
                crate::report::StepBoundary::new(notice.limit(), notice.observed())
            else {
                return TargetFact::Errored {
                    on: name,
                    detail: "the engine supplied an invalid step boundary".to_owned(),
                };
            };
            TargetFact::StepLimitReached { on: name, boundary }
        }
        MutantConclusion::Errored
        | MutantConclusion::Inconclusive
        | MutantConclusion::Unobserved
        | MutantConclusion::NotRun => TargetFact::Errored {
            on: name,
            detail: format!(
                "the harness answered {}: {}",
                result.outcome().name(),
                tail(&result.output)
            ),
        },
    }
}

/// The pair: the original must pass right now, and the kill must reproduce.
fn confirm(
    judging: &Judging<'_>,
    asked: (&Mutant, &str),
    request: &Request,
    expected: ExpectedReproduction,
) -> Result<Result<(), Unconfirmed>, crate::error::RunnerError> {
    let (mutant, on) = asked;
    let control = judging
        .controls
        .ask(judging.subject, request, judging.watch)?;
    let faulted = judging.subject.perturbing == Perturbing::Faults;
    if faulted {
        judging
            .watch
            .trace
            .fault_control(crate::trace::FaultControlRecord {
                fault: mutant.display_id.to_string(),
                target: on.to_owned(),
                passed: control.is_none(),
            });
    }
    if let Some(failure) = control {
        return Ok(Err(Unconfirmed::ControlFailed { detail: failure }));
    }
    let second = judging
        .subject
        .session
        .exec(request, judging.watch.cancel)?;
    if faulted {
        let milliseconds = second.duration.as_millis();
        let duration_ms = u64::try_from(milliseconds).map_err(|_outside_wire_range| {
            crate::assure::run::RunInvariantError::MutationDurationOutsideWire { milliseconds }
        })?;
        judging
            .watch
            .trace
            .fault_exec(crate::trace::FaultExecRecord {
                fault: mutant.display_id.to_string(),
                role: crate::trace::FaultRole::Confirmation,
                target: on.to_owned(),
                args: request.args.clone(),
                outcome: second.outcome().name().to_owned(),
                duration_ms,
                alone: false,
            });
    }
    Ok(expected.compare(second.outcome()))
}

/// The exact execution fact a confirmation run must reproduce.
///
/// Kept separate from [`Outcome`]: only these two facts have a confirmation protocol, so a finite step boundary or harness error cannot accidentally become a successful reproduction through a broad predicate such as `detected()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedReproduction {
    Killed,
    Waited,
}

impl ExpectedReproduction {
    const fn compare(self, actual: Outcome) -> Result<(), Unconfirmed> {
        match self {
            Self::Killed => match actual {
                Outcome::Killed => Ok(()),
                Outcome::NotRun
                | Outcome::Survived
                | Outcome::StepLimitReached
                | Outcome::Waited
                | Outcome::Inconclusive
                | Outcome::Errored => Err(Unconfirmed::DidNotReproduce),
            },
            Self::Waited => match actual {
                Outcome::Waited => Ok(()),
                Outcome::NotRun
                | Outcome::Killed
                | Outcome::Survived
                | Outcome::StepLimitReached
                | Outcome::Inconclusive
                | Outcome::Errored => Err(Unconfirmed::DidNotReproduce),
            },
        }
    }
}

/// What the original code says about each test, asked once per test however many mutations want to know at once.
#[derive(Debug, Default)]
struct Controls {
    /// One slot per question, which the first asker fills while every other asker of it waits.
    asked: Mutex<BTreeMap<ControlKey, Arc<Mutex<Option<Original>>>>>,
    /// What each control established about its target's baseline reach, by the mutation whose kill it confirmed.
    observed: Mutex<BTreeMap<String, Vec<Drift>>>,
}

/// What a control of the original code came to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Original {
    /// Every selected test passed.
    Passed,
    /// Something else, and what it said.
    Failed(String),
}

/// The two independent optional selectors that identify one pristine control.
/// Keeping them separate prevents delimiter collisions from aliasing controls.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ControlKey {
    target: Option<String>,
    test: Option<String>,
}

impl ControlKey {
    fn of(request: &Request) -> Self {
        Self {
            target: request.target.clone(),
            test: request.test.clone(),
        }
    }
}

impl Controls {
    /// Why this test fails on the original, or nothing when it passes, running the control once for every asker of the same question.
    fn ask(
        &self,
        subject: Subject<'_>,
        request: &Request,
        watch: Watch<'_>,
    ) -> Result<Option<String>, crate::error::RunnerError> {
        let session = subject.session;
        let slot = Arc::clone(
            self.asked
                .lock()
                .map_err(|_poisoned| schedule::ScheduleError::ControlStatePoisoned)?
                .entry(ControlKey::of(request))
                .or_default(),
        );
        let mut answer = slot
            .lock()
            .map_err(|_poisoned| schedule::ScheduleError::ControlStatePoisoned)?;
        let original = match answer.as_ref() {
            Some(known) => known.clone(),
            None => {
                let control = session.control(request, watch.cancel, Observing::Reach)?;
                if subject.perturbing == Perturbing::Mutants {
                    self.observe(request, &control.observed, watch)?;
                }
                let original = if control.result.outcome() == Outcome::Survived {
                    Original::Passed
                } else {
                    Original::Failed(format!(
                        "{}: {}",
                        control.result.outcome().name(),
                        tail(&control.result.output)
                    ))
                };
                *answer = Some(original.clone());
                original
            }
        };
        drop(answer);
        Ok(match original {
            Original::Passed => None,
            Original::Failed(failure) => Some(failure),
        })
    }

    /// Keeps what one control established about each target's baseline reach under the mutation it was confirming, and says so in the recording.
    fn observe(
        &self,
        request: &Request,
        observed: &[rust_mutants::session::Observed],
        watch: Watch<'_>,
    ) -> Result<(), crate::error::RunnerError> {
        let drift: Vec<Drift> = observed
            .iter()
            .map(|one| Drift::of(&one.target, &one.steadiness))
            .collect();
        for one in &drift {
            watch.trace.drift(crate::trace::DriftRecord {
                mutant: Some(request.mutant.clone()),
                observed: one.clone(),
            });
        }
        self.observed
            .lock()
            .map_err(|_poisoned| schedule::ScheduleError::ControlStatePoisoned)?
            .entry(request.mutant.clone())
            .or_default()
            .extend(drift);
        Ok(())
    }

    /// What the controls run while judging `mutant` established, taken so that it is recorded once.
    fn taken(&self, mutant: &str) -> Result<Vec<Drift>, crate::error::RunnerError> {
        Ok(self
            .observed
            .lock()
            .map_err(|_poisoned| schedule::ScheduleError::ControlStatePoisoned)?
            .remove(mutant)
            .unwrap_or_default())
    }
}

/// One mutation execution and what it was.
struct Ran<'a> {
    mutant: &'a Mutant,
    measured: Option<&'a Measured>,
    request: &'a Request,
    result: &'a MutantResult,
    /// Whether the machine was given to it, which a run does once when a budget expires.
    alone: bool,
    /// Which record the execution is.
    perturbing: Perturbing,
}

/// One mutation execution, as the recording holds it.
fn record_exec(
    watch: Watch<'_>,
    ran: &Ran<'_>,
) -> Result<(), crate::assure::run::RunInvariantError> {
    let milliseconds = ran.result.duration.as_millis();
    let duration_ms = u64::try_from(milliseconds).map_err(|_outside_wire_range| {
        crate::assure::run::RunInvariantError::MutationDurationOutsideWire { milliseconds }
    })?;
    let target = ran
        .measured
        .map_or_else(|| SUITE.to_owned(), |one| one.target.name());
    match ran.perturbing {
        Perturbing::Mutants => watch.trace.mutant_exec(crate::trace::MutantExecRecord {
            mutant: ran.mutant.display_id.to_string(),
            target,
            args: ran.request.args.clone(),
            outcome: ran.result.outcome().name().to_owned(),
            step_boundary: ran.result.step_notice().and_then(|notice| {
                crate::report::StepBoundary::new(notice.limit(), notice.observed())
            }),
            duration_ms,
            alone: ran.alone,
        }),
        Perturbing::Faults => watch.trace.fault_exec(crate::trace::FaultExecRecord {
            fault: ran.mutant.display_id.to_string(),
            role: crate::trace::FaultRole::First,
            target,
            args: ran.request.args.clone(),
            outcome: ran.result.outcome().name().to_owned(),
            duration_ms,
            alone: ran.alone,
        }),
    }
    Ok(())
}

/// Whether an expired budget has a quiet measurement coming to it.
#[must_use]
pub const fn quiet_measurement_due(outcome: Outcome, cancelled: bool) -> bool {
    matches!(outcome, Outcome::Waited) && !cancelled
}

/// The request the pair confirmation is made with: the one that ran, or the target the package suite found the answer in.
#[must_use]
pub fn narrowed(request: Request, measured: Option<&Measured>, answered: &str) -> Request {
    if measured.is_some() || answered.is_empty() {
        return request;
    }
    request.with_target(answered.to_owned()).test(None)
}

/// The name a route that ran the whole package suite answers to, in a recording and in a report.
pub const SUITE: &str = "package-suite";

/// The request that runs one mutant against one test, or against every test the session prepared when no proof says which could notice it.
#[must_use]
pub fn request_for(mutant: &str, measured: Option<&Measured>, args: &[String]) -> Request {
    let request = Request::new(mutant).with_args(args.to_vec());
    match measured {
        Some(one) => request.with_target(one.target.name()),
        None => request,
    }
}

/// The last line worth quoting from a capture.
#[must_use]
pub fn tail(output: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(output) else {
        return rust_mutants::telling::LosslessBytes::new(output).to_string();
    };
    match text.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(line) => line.chars().take(200).collect(),
        None => "(it said nothing)".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnsweredIndex, ExpectedReproduction, Outcome, TargetObservation, Unconfirmed, Unsettled,
        select_unsettled,
    };
    use rust_mutants::session::Request;

    #[test]
    fn confirmation_reproduces_the_same_fact_and_no_other_outcome() {
        for (expected, reproduced) in [
            (ExpectedReproduction::Killed, Outcome::Killed),
            (ExpectedReproduction::Waited, Outcome::Waited),
        ] {
            for actual in Outcome::ALL {
                assert_eq!(
                    expected.compare(actual),
                    if actual == reproduced {
                        Ok(())
                    } else {
                        Err(Unconfirmed::DidNotReproduce)
                    },
                    "{expected:?} must not be confirmed by {actual:?}"
                );
            }
        }
    }

    fn selected(facts: Vec<Unsettled>) -> (TargetObservation, String) {
        let aggregate = facts
            .iter()
            .fold(TargetObservation::Survived, |joined, fact| {
                joined.join(fact.observation())
            });
        let selected = select_unsettled(facts, aggregate).expect("one non-answer");
        (selected.observation(), selected.on().to_owned())
    }

    fn step(on: &str) -> Unsettled {
        Unsettled::StepLimitReached {
            on: on.to_owned(),
            boundary: crate::report::StepBoundary::new(10, 11).expect("valid boundary"),
        }
    }

    fn waited(on: &str) -> Unsettled {
        Unsettled::Waited {
            on: on.to_owned(),
            retry: Request::new("mutant"),
            answered: AnsweredIndex(0),
        }
    }

    fn errored(on: &str) -> Unsettled {
        Unsettled::Errored {
            on: on.to_owned(),
            detail: "apparatus".to_owned(),
        }
    }

    fn unconfirmed(on: &str) -> Unsettled {
        Unsettled::Unconfirmed {
            on: on.to_owned(),
            why: Unconfirmed::DidNotReproduce,
        }
    }

    #[test]
    fn a_later_survival_cannot_erase_any_non_answer() {
        for (fact, expected) in [
            (
                step("step"),
                (TargetObservation::StepLimitReached, "step".to_owned()),
            ),
            (
                waited("waited"),
                (TargetObservation::Waited, "waited".to_owned()),
            ),
            (
                unconfirmed("unconfirmed"),
                (TargetObservation::Unconfirmed, "unconfirmed".to_owned()),
            ),
            (
                errored("errored"),
                (TargetObservation::Errored, "errored".to_owned()),
            ),
        ] {
            assert_eq!(selected(vec![fact]), expected);
        }
    }

    #[test]
    fn non_answers_aggregate_independently_of_target_order() {
        let forward = selected(vec![step("z-step"), waited("y-wait"), errored("x-error")]);
        let reverse = selected(vec![errored("x-error"), waited("y-wait"), step("z-step")]);
        assert_eq!(forward, reverse);
        assert_eq!(forward, (TargetObservation::Errored, "x-error".to_owned()));

        assert_eq!(
            selected(vec![waited("z"), waited("a")]),
            selected(vec![waited("a"), waited("z")])
        );
        assert_eq!(
            selected(vec![waited("z"), waited("a")]),
            (TargetObservation::Waited, "a".to_owned())
        );
    }
}
