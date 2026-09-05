// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asking, of every mutation the compiler accepted, whether any test would notice it.

use std::collections::{BTreeMap, BTreeSet};

use rust_mutants::catalog::Mutant;
use rust_mutants::outcome::Outcome;
use rust_mutants::session::{Request, Session};

use crate::assure::baseline::Measured;
use crate::assure::route::{self, Route};
use crate::coverage::Block;
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
    /// A test ran out of time twice with it active, which is a behaviour change the tests noticed.
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

    /// Whether the tests caught this mutation.
    #[must_use]
    pub const fn caught(&self) -> bool {
        matches!(self, Self::Killed { .. } | Self::TimedOut { .. })
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
                }
                Disposition::TimedOut { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                    counts.timed_out = counts.timed_out.saturating_add(1);
                }
                Disposition::Survived { .. } => {
                    counts.executed = counts.executed.saturating_add(1);
                    counts.survived = counts.survived.saturating_add(1);
                    if accepted.contains(&judged.id) {
                        counts.accepted = counts.accepted.saturating_add(1);
                    }
                }
                Disposition::Unreached => {
                    counts.unreached = counts.unreached.saturating_add(1);
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
            .filter(|judged| !accepted.contains(&judged.id))
            .filter_map(finding_of)
            .collect()
    }
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
        Disposition::Killed { .. }
        | Disposition::TimedOut { .. }
        | Disposition::Rejected { .. } => {
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
    let (session, baseline) = (subject.session, subject.baseline);
    let phase = watch.trace.phase("mutation");
    let mut mutation = Mutation::default();
    for skip in session.skips() {
        let count = mutation
            .skips
            .entry(skip.reason.name().to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
    }

    let catalog = session.catalog();
    let rejected: BTreeMap<&str, &str> = session
        .rejections()
        .iter()
        .map(|rejection| (rejection.id.as_str(), rejection.diagnostic.as_str()))
        .collect();
    let mutants: Vec<&Mutant> = catalog.mutants().iter().collect();
    let total = u64::try_from(mutants.len()).unwrap_or(u64::MAX);
    let mut controls = Controls::default();
    let mut judging = Judging {
        subject,
        options,
        controls: &mut controls,
        watch,
    };

    for (index, mutant) in mutants.iter().enumerate() {
        let done = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        notes.progress(&mutant.display_id, done, total);

        let position = session.position(mutant).map(|at| crate::report::Position {
            line: at.line,
            column: at.byte_column,
            character_column: at.char_column,
        });
        let disposition = if let Some(diagnostic) = rejected.get(mutant.id.as_str()) {
            Disposition::Rejected {
                diagnostic: (*diagnostic).to_owned(),
            }
        } else {
            let route = route::route(
                &mutant.candidate.path,
                position.map(|at| crate::coverage::Point {
                    line: at.line,
                    column: at.column,
                }),
                baseline,
                &options.instrumented,
            );
            watch.trace.note(
                "route",
                &format!(
                    "{} {} {} targets",
                    mutant.display_id,
                    route.granularity(),
                    route.reaching().len()
                ),
            );
            judge(&mut judging, mutant, route)?
        };

        mutation.judged.push(Judged {
            id: mutant.id.clone(),
            display_id: mutant.display_id.clone(),
            path: mutant.candidate.path.clone(),
            rule: mutant.candidate.rule.to_string(),
            position,
            disposition,
        });
    }
    phase.end();
    Ok(mutation)
}

/// Runs one mutant against the tests its route named, stopping at the first confirmed catch.
struct Judging<'a> {
    subject: Subject<'a>,
    options: &'a MutationOptions,
    controls: &'a mut Controls,
    watch: Watch<'a>,
}

fn judge(
    judging: &mut Judging<'_>,
    mutant: &Mutant,
    route: Route,
) -> Result<Disposition, crate::error::RunnerError> {
    let (session, baseline) = (judging.subject.session, judging.subject.baseline);
    let (options, watch) = (judging.options, judging.watch);
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
    controls: &mut Controls,
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
    asked: BTreeMap<String, Option<String>>,
}

impl Controls {
    /// Why this test fails on the original, or nothing when it passes.
    fn ask(
        &mut self,
        session: &Session,
        request: &Request,
        watch: Watch<'_>,
    ) -> Result<Option<String>, crate::error::RunnerError> {
        let key = format!(
            "{}::{}",
            request.target.as_deref().unwrap_or_default(),
            request.test.as_deref().unwrap_or_default()
        );
        if let Some(known) = self.asked.get(&key) {
            return Ok(known.clone());
        }
        let control = session.control(request, watch.cancel)?;
        let failure = (control.outcome != Outcome::Survived)
            .then(|| format!("{}: {}", control.outcome.name(), tail(&control.output)));
        self.asked.insert(key, failure.clone());
        Ok(failure)
    }
}

/// The request that runs one mutant against one test.
fn request_for(mutant: &Mutant, measured: &Measured, args: &[String]) -> Request {
    Request {
        mutant: mutant.id.clone(),
        target: Some(format!(
            "{}/{}/{}",
            measured.target.package,
            measured.target.unit.name(),
            measured.target.unit_name
        )),
        test: (!measured.target.is_whole_binary()).then(|| measured.target.path.clone()),
        args: args.to_vec(),
        timeout: None,
    }
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
