// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asking, of every mutation the compiler accepted, whether any test would notice it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

use rust_mutants::catalog::Mutant;
use rust_mutants::outcome::Outcome;
use rust_mutants::session::{Request, Session};

use crate::assure::baseline::Measured;
use crate::assure::route::{self, Route};
use crate::assure::schedule;
use crate::coverage::Block;
use crate::evidence::store;
use crate::report::{Finding, FindingKind, MutantAccounting, TargetStatus};
use crate::ui::Notes;
use crate::watch::Watch;

/// Why a kill could not be believed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    /// The wire name a report uses.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ControlFailed { .. } => "flaky-mutation-control",
            Self::DidNotReproduce => "flaky-mutation-kill",
        }
    }

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
#[non_exhaustive]
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
    /// A test ran out of time with it active, which establishes nothing either way.
    TimedOut {
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
    /// The wire name a report records.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Rejected { .. } => "compile-rejected",
            Self::Killed { .. } => "killed",
            Self::TimedOut { .. } => "timed_out",
            Self::Survived { .. } => "survived",
            Self::Unreached => "unreached",
            Self::Unconfirmed { .. } => "unconfirmed",
            Self::Errored { .. } => "errored",
        }
    }

    /// The test that decided it, when one did.
    #[must_use]
    pub fn decided_by(&self) -> Option<&str> {
        match self {
            Self::Killed { by } => Some(by),
            Self::TimedOut { on } | Self::Unconfirmed { on, .. } | Self::Errored { on, .. } => {
                Some(on)
            }
            Self::Rejected { .. } | Self::Survived { .. } | Self::Unreached => None,
        }
    }
}

/// One mutant and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The rule that proposed it.
    pub rule: String,
    /// Where it is, when the catalog could say.
    pub position: Option<crate::report::Position>,
    /// What was established.
    pub disposition: Disposition,
    /// The run that established it, when it was not this one.
    pub source_run_id: Option<String>,
}

/// What the mutation phase established, whole.
#[derive(Debug, Clone, Default)]
pub struct Mutation {
    /// Every mutant, in catalog order.
    pub judged: Vec<Judged>,
    /// What every phase of the engine skipped, by reason, for the report's limitations.
    pub skips: BTreeMap<String, u64>,
}

impl Mutation {
    /// The counts, folded from the dispositions.
    #[must_use]
    pub fn accounting(&self, accepted: &BTreeSet<String>) -> MutantAccounting {
        let mut counts = MutantAccounting {
            cataloged: u32::try_from(self.judged.len()).unwrap_or(u32::MAX),
            ..MutantAccounting::default()
        };
        for judged in &self.judged {
            match &judged.disposition {
                Disposition::Rejected { .. } => {
                    counts.rejected = counts.rejected.saturating_add(1);
                }
                Disposition::Killed { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                    counts.killed = counts.killed.saturating_add(1);
                    if judged.source_run_id.is_some() {
                        counts.reused_killed = counts.reused_killed.saturating_add(1);
                    }
                }
                Disposition::TimedOut { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                    counts.timed_out = counts.timed_out.saturating_add(1);
                }
                Disposition::Survived { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                    counts.survived = counts.survived.saturating_add(1);
                    if judged.source_run_id.is_some() {
                        counts.reused_survived = counts.reused_survived.saturating_add(1);
                    }
                    if accepted.contains(&judged.id) {
                        counts.accepted = counts.accepted.saturating_add(1);
                    }
                }
                Disposition::Unreached => {
                    counts.unreached = counts.unreached.saturating_add(1);
                    if accepted.contains(&judged.id) {
                        counts.accepted = counts.accepted.saturating_add(1);
                    }
                }
                Disposition::Unconfirmed { .. } | Disposition::Errored { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                }
            }
        }
        counts
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

/// Whether a reviewer's acceptance answers for this mutation.
///
/// It answers for a mutation nothing noticed and for nothing else. An outcome
/// that established nothing either way — a pair that did not agree, a harness
/// that could not run, a budget that expired — is not a decision anybody can
/// sign off, and letting an acceptance remove its finding would turn "I could
/// not tell" into "I looked and it is fine".
fn answered_by(judged: &Judged, accepted: &BTreeSet<String>) -> bool {
    matches!(
        judged.disposition,
        Disposition::Survived { .. } | Disposition::Unreached
    ) && accepted.contains(&judged.id)
}

/// The finding one disposition raises, if it raises one.
fn finding_of(judged: &Judged) -> Option<Finding> {
    let (kind, detail) = match &judged.disposition {
        Disposition::Survived { route } => (
            FindingKind::SurvivingMutant,
            format!(
                "no test noticed {} at {}:{}; {} {} could and did not",
                judged.rule,
                judged.path,
                judged
                    .position
                    .map_or_else(|| "?".to_owned(), |at| at.line.to_string()),
                route.reaching().len(),
                if route.reaching().len() == 1 {
                    "test"
                } else {
                    "tests"
                },
            ),
        ),
        Disposition::Unreached => (
            FindingKind::SurvivingMutant,
            format!(
                "no measured test reaches {} at {}: the mutation lives in code the tests \
                 never execute",
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
        Disposition::TimedOut { on } => (
            FindingKind::Timeout,
            format!(
                "{on} ran out of time with {} at {} active: an expired budget establishes \
                 nothing about the mutation",
                judged.rule, judged.path
            ),
        ),
        Disposition::Killed { .. } | Disposition::Rejected { .. } => {
            return None;
        }
    };
    let mut finding = Finding::new(kind, &judged.display_id, &detail);
    finding.position = judged.position;
    Some(finding)
}

/// What is being measured: the prepared engine session and what the
/// baseline saw.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// The prepared session.
    pub session: &'a Session,
    /// Every target the baseline measured.
    pub baseline: &'a [Measured],
}

/// What to measure and how.
#[derive(Debug, Clone)]
pub struct MutationOptions {
    /// The regions the coverage build described, which tells "nothing reached this" apart from "the measurement says nothing".
    pub instrumented: BTreeSet<Block>,
    /// The mutants a reviewer accepted with a reason.
    pub accepted: BTreeSet<String>,
    /// Arguments for the test binaries.
    pub test_args: Vec<String>,
    /// What earlier runs established about individual mutants, and what this run knows of the targets they name. `None` for a run that establishes everything itself.
    pub evidence: Option<Evidence>,
    /// How many mutations to measure at once. Zero takes the processors the machine offers, capped.
    pub jobs: u32,
    /// Whether a resource only one test may hold at a time forces the run to measure one mutation at a time.
    pub exclusive: bool,
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
    /// What a person calls each of those targets, by identity. A record names identities, because a name is what a reader reads and an identity is what a route decides.
    pub names: BTreeMap<String, String>,
}

impl Evidence {
    /// The identity of the target a person calls `name`.
    fn identity(&self, name: &str) -> Option<&str> {
        self.names
            .iter()
            .find(|(_id, called)| called.as_str() == name)
            .map(|(id, _called)| id.as_str())
    }
}

/// Runs every accepted mutant against the tests that could notice it.
///
/// # Errors
/// The engine's refusals. A mutant that survives is not an error: it is the
/// finding.
pub fn run(
    subject: Subject<'_>,
    options: &MutationOptions,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<Mutation, crate::error::RunnerError> {
    let mut nothing = |_judged: &Judged| {};
    run_resuming(
        subject,
        options,
        &mut Resume {
            state: None,
            record: &mut nothing,
        },
        crate::assure::baseline::Reporting { notes, watch },
    )
}

/// What an interrupted run already judged, and where to record what this one judges.
#[expect(
    missing_debug_implementations,
    reason = "a recorder is a closure the caller owns; there is nothing to print about one"
)]
pub struct Resume<'a> {
    /// The state that run left, or nothing for a run starting cold.
    pub state: Option<&'a crate::checkpoint::State>,
    /// Called with each mutant as it finishes, so the caller can save what has been established before it can be lost.
    pub record: &'a mut dyn FnMut(&Judged),
}

/// [`run`], continuing from what an interrupted run had already judged.
///
/// Only a kill and a confirmed timeout are inherited: both are existential
/// claims about this exact tree, and a named test noticing a mutant stays true
/// however the next run routes. Everything else is re-derived.
///
/// # Errors
/// See [`run`].
pub fn run_resuming(
    subject: Subject<'_>,
    options: &MutationOptions,
    resume: &mut Resume<'_>,
    reporting: crate::assure::baseline::Reporting<'_, '_>,
) -> Result<Mutation, crate::error::RunnerError> {
    let crate::assure::baseline::Reporting { notes, watch } = reporting;
    let (session, baseline) = (subject.session, subject.baseline);
    let phase = watch.trace.phase("mutation-judge");
    let mut mutation = Mutation::default();
    for skip in session.skips() {
        let count = mutation
            .skips
            .entry(skip.reason.name().to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
    }

    record_probe(watch, session, baseline);

    let catalog = session.catalog();
    let rejected: BTreeMap<&str, &str> = session
        .rejections()
        .iter()
        .map(|rejection| (rejection.id.as_str(), rejection.diagnostic.as_str()))
        .collect();
    let mutants: Vec<&Mutant> = catalog.mutants().iter().collect();
    let total = u64::try_from(mutants.len()).unwrap_or(u64::MAX);
    let controls = Controls::default();
    let judging = Judging {
        subject,
        options,
        controls: &controls,
        watch,
        infected: infections(baseline, &session.probed().infected),
    };

    let measured = schedule::measure(
        &mutants,
        schedule::workers(options.jobs, schedule::available(), options.exclusive),
        |_at, mutant| establish(mutant, &judging, resume.state, &rejected),
    );

    for (index, answer) in measured.into_iter().enumerate() {
        let judged = answer?;
        let done = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        notes.progress(&judged.display_id, done, total);
        (resume.record)(&judged);
        mutation.judged.push(judged);
    }
    phase.end();
    Ok(mutation)
}

/// What one mutant comes to, without committing anything a report will carry.
///
/// Workers call this at the same time as one another, so it reads what the run
/// already holds and writes nothing but the evidence store and the trace, both
/// of which take their own locks. The caller commits the answers in catalog
/// order.
fn establish(
    mutant: &Mutant,
    judging: &Judging<'_>,
    state: Option<&crate::checkpoint::State>,
    rejected: &BTreeMap<&str, &str>,
) -> Result<Judged, crate::error::RunnerError> {
    let (session, baseline, options, watch) = (
        judging.subject.session,
        judging.subject.baseline,
        judging.options,
        judging.watch,
    );
    let position = session.position(mutant).map(|at| crate::report::Position {
        line: at.line,
        column: at.byte_column,
        character_column: at.char_column,
    });
    let mut source: Option<String> = None;
    let disposition = if let Some(saved) = state
        .and_then(|state| state.mutant(&mutant.id))
        .and_then(inherited)
    {
        saved
    } else if let Some(diagnostic) = rejected.get(mutant.id.as_str()) {
        Disposition::Rejected {
            diagnostic: (*diagnostic).to_owned(),
        }
    } else {
        let route = routed(mutant, position, baseline, judging);
        let reused = reuse(options, &route, &mutant.id);
        record_route(
            watch,
            mutant,
            &route,
            reused.as_ref().map(|(_, run)| run.as_str()),
        );
        if let Some((disposition, run_id)) = reused {
            source = Some(run_id);
            disposition
        } else {
            let established = judge(judging, mutant, route.clone())?;
            keep(options, &mutant.id, &route, &established);
            established
        }
    };
    Ok(Judged {
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        path: mutant.candidate.path.clone(),
        rule: mutant.candidate.rule.to_string(),
        position,
        disposition,
        source_run_id: source,
    })
}

/// The tests that could notice this mutant, and a note in the trace saying how they were chosen.
fn routed(
    mutant: &Mutant,
    position: Option<crate::report::Position>,
    baseline: &[Measured],
    judging: &Judging<'_>,
) -> Route {
    let mut route = route::route(
        &mutant.candidate.path,
        position.map(|at| crate::coverage::Point {
            line: at.line,
            column: at.column,
        }),
        baseline,
        &judging.options.instrumented,
    );
    let probed = judging.subject.session.probed();
    if probed.asked.contains(&mutant.index) {
        route = route::uninfected(route, mutant.index, &judging.infected);
    }
    if let Some(proof) = judging.subject.session.branch(mutant.index) {
        route = route::discharge(
            route,
            &route::Proven {
                path: &mutant.candidate.path,
                body: route::Body {
                    start: crate::coverage::Point {
                        line: proof.body_start.line,
                        column: proof.body_start.byte_column,
                    },
                    end: crate::coverage::Point {
                        line: proof.body_end.line,
                        column: proof.body_end.byte_column,
                    },
                },
                baseline,
                instrumented: &judging.options.instrumented,
            },
        );
    }
    route
}

/// Records what the probe pass measured for each target the baseline ran.
///
/// A target the pass did not measure carries no facts at all, and none is not
/// zero: a reader who cannot tell "infected nothing" from "was never asked"
/// cannot tell a discharge that rests on evidence from one that rests on
/// silence.
fn record_probe(watch: Watch<'_>, session: &Session, baseline: &[Measured]) {
    let probed = session.probed();
    if probed.asked.is_empty() {
        return;
    }
    let infected = infections(baseline, &probed.infected);
    for measured in baseline {
        let seen = infected.get(&measured.target.id);
        watch.trace.probe_exec(crate::trace::ProbeExecRecord {
            target: measured.target.id.clone(),
            outcome: if seen.is_some() {
                "measured".to_owned()
            } else {
                "not-measured".to_owned()
            },
            infected: seen.map(|one| u64::try_from(one.len()).unwrap_or(u64::MAX)),
        });
    }
}

/// Records how one mutant's tests were chosen, and whether this run established the answer itself.
fn record_route(watch: Watch<'_>, mutant: &Mutant, route: &Route, reused: Option<&str>) {
    watch.trace.route(crate::trace::RouteRecord {
        mutant: mutant.display_id.clone(),
        granularity: route.granularity().to_owned(),
        fallback: route.fallback().map(|why| why.name().to_owned()),
        reaching: route.reaching().to_vec(),
        discharged: route
            .discharged()
            .iter()
            .map(|one| crate::trace::DischargeRecord {
                target: one.target.clone(),
                proof: one.proof.to_owned(),
            })
            .collect(),
        file_candidates: u64::try_from(route.file_candidates()).unwrap_or(u64::MAX),
        reused: reused.map(ToOwned::to_owned),
    });
}

/// What an earlier run established about this mutant, when this run may believe it.
fn reuse(options: &MutationOptions, route: &Route, mutant: &str) -> Option<(Disposition, String)> {
    let evidence = options.evidence.as_ref()?;
    let record = store::read(&evidence.root, mutant).ok()??;
    let reaching: BTreeSet<String> = route.reaching().iter().cloned().collect();
    record.believable(&reaching, &evidence.standing).ok()?;
    let disposition = match &record.outcome {
        store::Outcome::Killed { target, .. } => Disposition::Killed {
            by: evidence
                .names
                .get(target)
                .cloned()
                .unwrap_or_else(|| target.clone()),
        },
        store::Outcome::Survived { .. } => Disposition::Survived {
            route: route.clone(),
        },
    };
    Some((disposition, record.run_id))
}

/// Records what this run established, for the next run of a tree these targets still behave the same in. Only a named kill and a survival are recorded: everything else is about the run rather than about the mutant.
fn keep(options: &MutationOptions, mutant: &str, route: &Route, disposition: &Disposition) {
    let Some(evidence) = options.evidence.as_ref() else {
        return;
    };
    let outcome = match disposition {
        Disposition::Killed { by } => {
            let Some(target) = evidence.identity(by) else {
                return;
            };
            let Some(key) = evidence.standing.passing.get(target) else {
                return;
            };
            store::Outcome::Killed {
                target: target.to_owned(),
                key: key.clone(),
            }
        }
        Disposition::Survived { .. } => {
            let mut targets = BTreeMap::new();
            for target in route.reaching() {
                let Some(key) = evidence.standing.passing.get(target) else {
                    return;
                };
                targets.insert(target.clone(), key.clone());
            }
            store::Outcome::Survived { targets }
        }
        _ => return,
    };
    drop(store::write(
        &evidence.root,
        &store::record(mutant, &evidence.run_id, outcome),
    ));
}

/// The disposition a checkpoint's record stands for, or nothing when this release does not inherit it.
fn inherited(saved: &crate::checkpoint::SavedMutant) -> Option<Disposition> {
    let by = saved.killed_by.clone()?;
    match saved.disposition.as_str() {
        "killed" => Some(Disposition::Killed { by }),
        "timed_out" => Some(Disposition::TimedOut { on: by }),
        _ => None,
    }
}

/// Runs one mutant against the tests its route named, stopping at the first confirmed catch.
struct Judging<'a> {
    subject: Subject<'a>,
    options: &'a MutationOptions,
    controls: &'a Controls,
    watch: Watch<'a>,
    infected: BTreeMap<String, BTreeSet<u32>>,
}

fn judge(
    judging: &Judging<'_>,
    mutant: &Mutant,
    route: Route,
) -> Result<Disposition, crate::error::RunnerError> {
    let (session, baseline) = (judging.subject.session, judging.subject.baseline);
    let (options, watch) = (judging.options, judging.watch);
    if let Route::Discharged { .. } = route {
        return Ok(Disposition::Survived { route });
    }
    if route.reaching().is_empty() {
        return Ok(Disposition::Unreached);
    }
    for target_id in route.reaching() {
        let Some(measured) = baseline
            .iter()
            .find(|measured| &measured.target.id == target_id)
        else {
            continue;
        };
        let request = request_for(mutant, measured, &options.test_args);
        let result = session.exec(&request, watch.cancel)?;
        watch.trace.mutant_exec(crate::trace::MutantExecRecord {
            mutant: mutant.display_id.clone(),
            target: measured.target.id.clone(),
            args: request.args.clone(),
            outcome: result.outcome.name().to_owned(),
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
        });
        match result.outcome {
            Outcome::Survived => {}
            Outcome::Killed | Outcome::TimedOut => {
                let name = measured.target.name();
                return Ok(match confirm(session, &request, judging.controls, watch)? {
                    Ok(()) if result.outcome == Outcome::TimedOut => {
                        Disposition::TimedOut { on: name }
                    }
                    Ok(()) => Disposition::Killed { by: name },
                    Err(why) => Disposition::Unconfirmed { on: name, why },
                });
            }
            _ => {
                return Ok(Disposition::Errored {
                    on: measured.target.name(),
                    detail: format!(
                        "the harness answered {}: {}",
                        result.outcome.name(),
                        tail(&result.output)
                    ),
                });
            }
        }
    }
    Ok(Disposition::Survived { route })
}

/// The pair: the original must pass right now, and the kill must reproduce.
fn confirm(
    session: &Session,
    request: &Request,
    controls: &Controls,
    watch: Watch<'_>,
) -> Result<Result<(), Unconfirmed>, crate::error::RunnerError> {
    if let Some(failure) = controls.ask(session, request, watch)? {
        return Ok(Err(Unconfirmed::ControlFailed { detail: failure }));
    }
    let second = session.exec(request, watch.cancel)?;
    if second.outcome.detected() {
        Ok(Ok(()))
    } else {
        Ok(Err(Unconfirmed::DidNotReproduce))
    }
}

/// What the original code says about each test, asked once per test.
#[derive(Debug, Default)]
struct Controls {
    /// The failure each test showed on the original, or nothing when it passed. Absent means it has not been asked yet.
    ///
    /// Workers share this, so it answers behind a lock the run never holds
    /// across a process: two of them asking the same question at once ask it
    /// twice and write the same answer, which costs a control and never a
    /// wrong one.
    asked: Mutex<BTreeMap<String, Option<String>>>,
}

impl Controls {
    /// Why this test fails on the original, or nothing when it passes.
    fn ask(
        &self,
        session: &Session,
        request: &Request,
        watch: Watch<'_>,
    ) -> Result<Option<String>, crate::error::RunnerError> {
        let key = format!(
            "{}::{}",
            request.target.as_deref().unwrap_or_default(),
            request.test.as_deref().unwrap_or_default()
        );
        if let Some(known) = self
            .asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return Ok(known.clone());
        }
        let control = session.control(request, watch.cancel)?;
        let failure = (control.outcome != Outcome::Survived)
            .then(|| format!("{}: {}", control.outcome.name(), tail(&control.output)));
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, failure.clone());
        Ok(failure)
    }
}

/// The request that runs one mutant against one test.
fn request_for(mutant: &Mutant, measured: &Measured, args: &[String]) -> Request {
    Request {
        mutant: mutant.id.clone(),
        target: Some(binary_of(measured)),
        test: (!measured.target.is_whole_binary()).then(|| measured.target.path.clone()),
        args: args.to_vec(),
        timeout: None,
    }
}

/// The engine's name for the binary this target is one test of: `package/kind/name`.
fn binary_of(measured: &Measured) -> String {
    format!(
        "{}/{}/{}",
        measured.target.package,
        measured.target.unit.name(),
        measured.target.unit_name
    )
}

/// What the probe pass measured, keyed by the target identities this run routes with.
///
/// The engine probes a binary: it runs each one once with nothing active and
/// records the mutants that binary infected. This run names one test of that
/// binary, so the two carry different names for different things, and a lookup
/// by the engine's name finds nothing for every target. Nothing reads as "the
/// probe says nothing about this target", which keeps every execution — a proof
/// layer switched on and never firing.
///
/// A binary that infected nothing is one whose every test infected nothing, so
/// giving each of its tests the binary's answer discharges only what the
/// measurement carries.
fn infections(
    baseline: &[Measured],
    infected: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    baseline
        .iter()
        .filter_map(|measured| {
            infected
                .get(&binary_of(measured))
                .map(|seen| (measured.target.id.clone(), seen.clone()))
        })
        .collect()
}

/// The last line worth quoting from a capture.
fn tail(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("(it said nothing)")
        .chars()
        .take(200)
        .collect()
}

/// Whether a measured target is one a mutation phase can use.
#[must_use]
pub fn usable(measured: &Measured) -> bool {
    measured.status == TargetStatus::Passed
}
