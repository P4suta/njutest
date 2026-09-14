// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving a session: judging every mutant it holds, and what a run shares between the ones it is measuring at once.

use std::sync::{PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::EngineError;
use crate::catalog::Mutant;
use crate::discover::SkipClaim;
use crate::outcome::Outcome;
use crate::runner::Cancel;
use crate::session::{Locator, Request, Session};
use crate::workspace::SessionError;

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

/// One mutant a reviewer declared, with the outcome the run must confirm.
///
/// A claim names its mutant either by identity, which is exact and moves when
/// anything in the file does, or by a locator, which says where the mutation
/// is and what it edits and survives an edit elsewhere. Never both: two ways
/// of naming one thing are two chances to name different things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// The mutant by identity or by a prefix that names exactly one.
    pub id: Option<String>,
    /// The mutant by where it is and what it edits.
    pub locator: Option<Locator>,
    /// Why the outcome is what it is. Required: an expectation without a reason is a suppression, and a report cannot audit one.
    pub reason: String,
    /// The outcome the run must confirm.
    pub outcome: Outcome,
}

impl Expectation {
    /// How the claim is written back to a reader.
    #[must_use]
    pub fn name(&self) -> String {
        match (&self.id, &self.locator) {
            (Some(id), _) => id.clone(),
            (None, Some(locator)) => format!(
                "{} {} {} {:?}",
                locator.path, locator.item, locator.rule, locator.original
            ),
            (None, None) => String::new(),
        }
    }
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
    /// Why it was never executed, when it was not.
    pub not_run_reason: Option<NotRunReason>,
    /// Which targets could have noticed it, and which of them ran.
    pub route: Option<crate::report::run::RouteDocument>,
    /// Whether this run measured it, rather than reusing what an earlier one established or never reaching it. A measurement records its own route.
    pub measured: bool,
    /// Whether the compiler renders the mutation identically to what it mutates, when the equivalence layer was asked.
    pub identical: Option<bool>,
}

/// Whether a reviewer's claim about one mutant held.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    /// The mutant it resolved to, when it resolved. The one that decided the standing, when it named several.
    pub mutant: Option<String>,
    /// How many mutants the claim was resolved against, which is one unless the locator stated a count.
    pub covered: u32,
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
    /// Every kind, in the order findings are reported.
    pub const ALL: [Self; 9] = [
        Self::SurvivingMutant,
        Self::InconclusiveMutant,
        Self::ErroredMutant,
        Self::NotRunMutant,
        Self::UnreachedMutant,
        Self::DischargedMutant,
        Self::StaleExpectation,
        Self::UnmatchedExpectation,
        Self::UnmatchedSkip,
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
            Self::DischargedMutant => "discharged-mutant",
            Self::StaleExpectation => "stale-expectation",
            Self::UnmatchedExpectation => "unmatched-expectation",
            Self::UnmatchedSkip => "unmatched-skip",
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
    /// How many candidate rows the run accounts for, excluding compiler refusals.
    ///
    /// An `unselected` row makes no compiler-acceptance claim: a scoped run
    /// deliberately leaves such a candidate unvalidated.
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
    /// How many of those never ran because a proof removed every target that could have noticed them.
    pub discharged: u32,
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
            if one.not_run_reason == Some(NotRunReason::Unreached) {
                tally.unreached = tally.unreached.saturating_add(1);
            }
            if one.not_run_reason == Some(NotRunReason::Discharged) {
                tally.discharged = tally.discharged.saturating_add(1);
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
                Outcome::NotRun if one.not_run_reason == Some(NotRunReason::Unreached) => {
                    FindingKind::UnreachedMutant
                }
                Outcome::NotRun if one.not_run_reason == Some(NotRunReason::Discharged) => {
                    FindingKind::DischargedMutant
                }
                Outcome::NotRun
                    if matches!(
                        one.not_run_reason,
                        Some(NotRunReason::Unselected | NotRunReason::StoppedEarly)
                    ) =>
                {
                    continue;
                }
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
                Standing::Met | Standing::Moved { .. } => {}
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
    /// The tree the equivalence layer builds and mutates, when a run asks it. `None` asks nothing.
    pub equivalence: Option<&'a Equivalence<'a>>,
    /// How many mutants to measure at once. Zero is [`jobs`]'s own answer.
    pub jobs: usize,
    /// Further arguments for the harness.
    pub args: &'a [String],
    /// Which part of the catalog this run is about. `None` is all of it.
    pub shard: Option<Shard>,
    /// Where what earlier runs of this exact tree established is kept, and this run's own name. `None` establishes everything afresh.
    pub outcomes: Option<Reusing<'a>>,
    /// Which of the catalog's mutants this run is about. `None` is every one the shard holds.
    pub filter: Option<&'a Filter>,
    /// Stop at the first finding rather than measuring the rest.
    pub fail_fast: bool,
}

/// Which of a catalog's mutants a run is about.
///
/// A filter narrows what a run measures and changes nothing about the
/// catalog: the digest is the catalog's, a stored outcome is still the same
/// tree's, and what a filter took out is reported as a mutant nobody selected
/// rather than left out of the accounting. A run that measured half a catalog
/// says so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Rules by name. Empty selects every rule.
    pub rules: Vec<String>,
    /// Families by name. Empty selects every family.
    pub families: Vec<String>,
    /// Rules never to select.
    pub skip_rules: Vec<String>,
    /// Families never to select.
    pub skip_families: Vec<String>,
    /// Paths, each with the lines of it the filter is about. Empty selects every file.
    pub files: Vec<(String, Option<(u32, u32)>)>,
    /// The identities, or prefixes of them, this run is about. `None` where nothing named any, which selects every mutant; a list that names none selects none, because a source of identities that came up empty is an answer rather than the absence of a question.
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

    /// Whether this run is about `mutant`, which sits at `line`.
    #[must_use]
    pub fn selects(&self, mutant: &Mutant, line: u32) -> bool {
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
            && !ids
                .iter()
                .any(|prefix| mutant.id.starts_with(prefix.as_str()))
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
            mutant: mutant.display_id.clone(),
            reason: NotRunReason::Unselected.name().to_owned(),
        });
        unselected.push(unexecuted(mutant, NotRunReason::Unselected));
    }
    observer.starting(count(places.len()));
    let judged = if jobs(options.jobs) == 1 {
        serially(session, &places, options, (cancel, observer))?
    } else {
        pool::judge(session, &places, options, (cancel, observer))?
    };
    observer.finished(started.elapsed());
    let mut judged = judged;
    judged.extend(unselected);
    judged.sort_by_key(|one| one.index);
    if let Some(asking) = options.equivalence {
        equivalence(session, asking, &mut judged, cancel);
    }
    let interrupted = judged
        .iter()
        .any(|one| one.not_run_reason == Some(NotRunReason::Interrupted));
    Ok(Run {
        judged,
        expectations: Vec::new(),
        skipped: session
            .skips()
            .iter()
            .fold(0u32, |total, skip| total.saturating_add(skip.count)),
        refused: count(session.rejections().len()),
        claims: session.claims().to_vec(),
        interrupted: interrupted || cancel.is_cancelled(),
        shard: options.shard,
        duration: started.elapsed(),
    })
}

/// The mutants a claim names, and where the first has moved to since the claim was written.
///
/// An identity names one. A locator names one unless it states a count, and
/// then it names that many: a reason written for a set of mutations is checked
/// against every one of them, so what comes back is the set rather than a
/// representative of it.
fn addressed<'s>(
    session: &'s Session,
    expectation: &Expectation,
) -> Result<(Vec<&'s Mutant>, Option<Standing>), String> {
    if let Some(id) = &expectation.id {
        return session
            .resolve(id)
            .map(|mutant| (vec![mutant], None))
            .map_err(|error| error.to_string());
    }
    let Some(locator) = &expectation.locator else {
        return Err("the claim names no mutant".to_owned());
    };
    let mutants = session
        .locate_all(locator)
        .map_err(|error| error.to_string())?;
    let moved = locator.line.zip(mutants.first()).and_then(|(from, first)| {
        let to = session.position(first)?.line;
        (to != from).then_some(Standing::Moved { from, to })
    });
    Ok((mutants, moved))
}

/// What the run says about every mutant one claim names, and which of them decided it.
///
/// The claim holds only when each of them came to the declared outcome. One
/// that did not is what the standing reports, because a reason written for
/// three mutations stops being a reason for any of them the moment one of the
/// three is killed: what covered it then is a test, and the claim would be
/// exempting the other two on the strength of that.
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
///
/// Only a survivor is asked: a mutation a test noticed is one the compiler
/// plainly rendered, and asking about it would pay a build for an answer the
/// run already has. What the layer says is `identical`, `differs`, or nothing
/// at all — never `equivalent`, which is a claim about behaviour that a
/// comparison of two binaries cannot make. Whatever it fails at leaves the
/// survivor a survivor.
fn equivalence(
    session: &Session,
    asking: &Equivalence<'_>,
    judged: &mut [Judged],
    cancel: &Cancel,
) {
    let survivors: Vec<usize> = judged
        .iter()
        .enumerate()
        .filter(|(_, one)| one.outcome == Outcome::Survived)
        .map(|(at, _)| at)
        .collect();
    if survivors.is_empty() {
        return;
    }
    let phase = session.trace().phase("equivalence");
    let opened =
        crate::equivalence::Prover::open(asking.root, &asking.options, cancel, session.trace());
    let Ok(mut prover) = opened else {
        phase.end();
        return;
    };
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
        let Ok(answer) = prover.identical(&mutant.candidate, cancel) else {
            break;
        };
        session.trace().identical(crate::trace::IdenticalRecord {
            index: one.index,
            identity: answer.name().to_owned(),
            detail: match answer {
                crate::equivalence::artifacts::Identity::NotEstablished(why) => {
                    Some(why.to_owned())
                }
                _ => None,
            },
        });
        if let Some(one) = judged.get_mut(at) {
            one.identical = match answer {
                crate::equivalence::artifacts::Identity::Identical => Some(true),
                crate::equivalence::artifacts::Identity::Differs => Some(false),
                crate::equivalence::artifacts::Identity::NotEstablished(_) => None,
            };
        }
    }
    drop(prover.close());
    phase.end();
}

/// What the equivalence layer needs: the tree the user wrote, and how it is built.
///
/// It is the project's own tree rather than the snapshot, because what is
/// compared is what the project's own `cargo test --no-run` produces, with
/// nothing instrumented in it.
#[derive(Debug)]
pub struct Equivalence<'a> {
    /// The source root, which is what a person would build.
    pub root: &'a std::path::Path,
    /// How the tree is copied, which cargo builds it, and what it is compiled as.
    pub options: crate::equivalence::ProveOptions,
}

/// How many mutants a run measures at once. Zero is the default: as many as the machine has, capped at four.
///
/// Each test binary runs its own tests on as many threads as the machine has,
/// so a run that started one process per core would have every process
/// contending with every other and would measure the contention. Four is the
/// number that keeps a machine busy without making a duration a fact about
/// the load.
///
/// The guards narrow most executions to the tests that reached the mutation,
/// and a process running one test does not use the machine the way one running
/// a whole suite does. The cap stays anyway, because the ones that fall back —
/// a target the record could not attribute, a set of tests that does not answer
/// on its own — still run everything, and a budget is five times a baseline
/// measured with the machine to itself. A person who knows their suite says
/// `--jobs`, and a run that says nothing keeps the answer that is right when
/// the fallback fires.
#[must_use]
pub fn jobs(configured: usize) -> usize {
    if configured > 0 {
        return configured;
    }
    std::thread::available_parallelism().map_or(1, |cores| cores.get().min(DEFAULT_JOBS))
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
    let total = count(places.len());
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
        observer.judged(&one, count(position).saturating_add(1), total);
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
    if let Some(one) = reuse(session, mutant, options) {
        return Ok(one);
    }
    let established = execute(session, mutant, options, cancel)?;
    keep(mutant, options, &established);
    Ok(established)
}

/// Measuring several mutants at once, and delivering each as it finishes.
///
/// Delivery is in completion order and never in catalog order: one mutant
/// that hangs for its whole budget would otherwise hold back every result
/// behind it, and a progress line, a stream, and a stop-at-the-first-finding
/// would all wait on it. The report is put back into catalog order when it is
/// written, because that is the order a reader compares two runs in.
mod pool {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::{
        EngineError, Judged, Mutant, NotRunReason, Observer, Options, Session, count, one_mutant,
        route, unexecuted,
    };
    use crate::runner::Cancel;

    /// What a worker hands the coordinator.
    enum Delivery {
        /// A worker claimed the mutant at this position and started it.
        Started(usize),
        /// A worker finished the mutant at this position.
        Judged(usize, Box<Judged>),
        /// A worker could not go on, and neither can the run.
        Failed(Box<EngineError>),
    }

    /// Judges every mutant with `jobs` of them in flight, delivering each as it finishes.
    pub(super) fn judge<O: Observer>(
        session: &Session,
        places: &[&Mutant],
        options: &Options<'_>,
        watching: (&Cancel, &mut O),
    ) -> Result<Vec<Judged>, EngineError> {
        let (cancel, observer) = watching;
        let total = count(places.len());
        let mut done: Vec<Option<Judged>> = (0..places.len()).map(|_| None).collect();
        let mut failure: Option<EngineError> = None;
        let mut completed: u32 = 0;
        let mut stopped = false;
        let stop = std::sync::atomic::AtomicBool::new(false);
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel::<Delivery>();

        std::thread::scope(|scope| {
            for _worker in 0..super::jobs(options.jobs) {
                let sender = sender.clone();
                let next = &next;
                let stop = &stop;
                let _handle = scope.spawn(move || {
                    loop {
                        let at = next.fetch_add(1, Ordering::SeqCst);
                        let Some(mutant) = places.get(at) else {
                            return;
                        };
                        if cancel.is_cancelled() || stop.load(Ordering::SeqCst) {
                            return;
                        }
                        if sender.send(Delivery::Started(at)).is_err() {
                            return;
                        }
                        let sent = match one_mutant(session, mutant, options, cancel) {
                            Ok(mut one) => {
                                route(session, mutant, &mut one);
                                sender.send(Delivery::Judged(at, Box::new(one)))
                            }
                            Err(error) => sender.send(Delivery::Failed(Box::new(error))),
                        };
                        if sent.is_err() {
                            return;
                        }
                    }
                });
            }
            drop(sender);
            for delivery in receiver {
                match delivery {
                    Delivery::Started(at) => {
                        if let Some(mutant) = places.get(at) {
                            observer.started(mutant);
                        }
                    }
                    Delivery::Judged(at, one) => {
                        completed = completed.saturating_add(1);
                        observer.judged(&one, completed, total);
                        stopped |= options.fail_fast && super::stops(&one);
                        if let Some(place) = done.get_mut(at) {
                            *place = Some(*one);
                        }
                        if stopped {
                            stop.store(true, Ordering::SeqCst);
                        }
                    }
                    Delivery::Failed(error) => {
                        cancel.cancel();
                        if failure.is_none() {
                            failure = Some(*error);
                        }
                    }
                }
            }
        });

        if let Some(error) = failure {
            return Err(error);
        }
        let unreached = if stopped {
            NotRunReason::StoppedEarly
        } else {
            NotRunReason::Interrupted
        };
        Ok(places
            .iter()
            .zip(done)
            .map(|(mutant, one)| one.unwrap_or_else(|| unexecuted(mutant, unreached)))
            .collect())
    }
}

/// Records which targets could have noticed this mutation and which of them ran.
///
/// One record per judged mutant, whatever became of it: a mutant nothing
/// reached leaves a route and no execution, and one an earlier run answered
/// for names that run rather than a target. The record is the only place a
/// reader can see a proof layer remove work, so it is written even when the
/// mutant was never started.
fn route(session: &Session, mutant: &Mutant, judged: &mut Judged) {
    if let Some(reason) = judged.not_run_reason
        && session.trace().is_enabled()
    {
        session.trace().select(crate::trace::SelectRecord {
            mutant: mutant.display_id.clone(),
            reason: reason.name().to_owned(),
        });
    }
    let decided = session.route(mutant);
    let executed = if judged.source_run_id.is_some() || judged.outcome == Outcome::NotRun {
        Vec::new()
    } else {
        decided.executed(&judged.target, judged.outcome.detected())
    };
    judged.route = Some(crate::report::run::route_document(
        &decided,
        executed.clone(),
    ));
    if !session.trace().is_enabled() || judged.measured {
        return;
    }
    let mut record = decided.record(mutant, executed);
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
    /// The run is about to judge `total` mutants.
    fn starting(&mut self, _total: u32) {}

    /// A mutant is about to be judged.
    fn started(&mut self, _mutant: &Mutant) {}

    /// A mutant has been judged. `completed` counts what has been delivered, of `total`.
    fn judged(&mut self, _judged: &Judged, _completed: u32, _total: u32) {}

    /// Every mutant has been judged, and the run took `duration`.
    fn finished(&mut self, _duration: Duration) {}
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
    /// A filter took it out of what this run was asked to measure.
    Unselected,
    /// The run stopped at the first finding, as it was asked to.
    StoppedEarly,
}

impl NotRunReason {
    /// Every reason, in the order a report's schema lists them.
    pub const ALL: [Self; 5] = [
        Self::Unreached,
        Self::Discharged,
        Self::Interrupted,
        Self::Unselected,
        Self::StoppedEarly,
    ];

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
        not_run_reason: not_run_because(result.outcome, &judgement.route),
        route: None,
        measured: true,
        identical: None,
        source_run_id: None,
    })
}

/// Why a mutant that was never executed was not, when it was not.
///
/// A mutation no measured target reaches is one nothing needs to run to find
/// out again. Anything else that ends without a result ended because the run
/// did: a process the runner never got a status from is a run somebody
/// stopped, and reading it as a mutation no test can notice would report a
/// finding nobody measured.
fn not_run_because(outcome: Outcome, route: &crate::session::Route) -> Option<NotRunReason> {
    if outcome != Outcome::NotRun {
        return None;
    }
    match route {
        crate::session::Route::Unreached { .. } => Some(NotRunReason::Unreached),
        crate::session::Route::Discharged { .. } => Some(NotRunReason::Discharged),
        _ => Some(NotRunReason::Interrupted),
    }
}

/// Whether this outcome is the one a run asked to stop at the first finding stops at.
///
/// A run stops at the first thing a reader has to act on, which is what a
/// finding is: a mutation nothing noticed, one nothing could decide, one
/// nothing reached. It does not stop at a kill, which is the run working.
const fn stops(one: &Judged) -> bool {
    match one.outcome {
        Outcome::Killed | Outcome::TimedOut => false,
        Outcome::Survived => !one.expected,
        Outcome::NotRun => matches!(
            one.not_run_reason,
            Some(NotRunReason::Unreached | NotRunReason::Discharged)
        ),
        _ => true,
    }
}

/// What a filter leaves of a catalog, and what it took out.
///
/// What a filter took out is a mutant nobody selected, not a mutant nobody
/// cataloged: it keeps its row and its reason, so a report of a narrowed run
/// still accounts for the whole of the catalog it was cut from.
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
                    mutant: mutant.display_id.clone(),
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
        filter.selects(mutant, line)
    })
}

/// What an earlier run of this exact tree established about this mutant, when a record answers for it.
fn reuse(session: &Session, mutant: &Mutant, options: &Options<'_>) -> Option<Judged> {
    let reusing = options.outcomes?;
    if !reusing.keyed.usable() {
        return None;
    }
    let key = reusing.keyed.key(&mutant.id);
    let found = reusing.store.get(&key, &mutant.id);
    if session.trace().is_enabled() {
        session.trace().cache(crate::trace::CacheRecord {
            mutant: mutant.display_id.clone(),
            key,
            hit: found.is_some(),
            source_run_id: found.as_ref().map(|(_, record)| record.run_id.clone()),
        });
    }
    let (outcome, record) = found?;
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
        not_run_reason: None,
        route: None,
        measured: false,
        identical: None,
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
    if !reusing.keyed.usable() {
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
        not_run_reason: Some(reason),
        route: None,
        measured: false,
        identical: None,
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
            let resolved = addressed(session, expectation);
            let (covered, mutant, standing) = match resolved {
                Err(why) => (0, None, Standing::Unmatched { why }),
                Ok((mutants, moved)) => {
                    let ids: Vec<String> = mutants.iter().map(|mutant| mutant.id.clone()).collect();
                    let (named, standing) = standing_of(judged, expectation.outcome, &ids);
                    let standing = match standing {
                        Standing::Met => moved.unwrap_or(Standing::Met),
                        other => other,
                    };
                    if matches!(standing, Standing::Met | Standing::Moved { .. }) {
                        for one in judged.iter_mut().filter(|one| ids.contains(&one.id)) {
                            one.expected = true;
                        }
                    }
                    (
                        u32::try_from(ids.len()).unwrap_or(u32::MAX),
                        named,
                        standing,
                    )
                }
            };
            Verified {
                id: expectation.name(),
                locator: expectation.locator.clone(),
                reason: expectation.reason.clone(),
                outcome: expectation.outcome,
                mutant,
                covered,
                standing,
            }
        })
        .collect()
}
