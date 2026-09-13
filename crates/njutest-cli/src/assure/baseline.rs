// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: what the one verified run of every target observed.
//!
//! The engine runs every target once with nothing active before it will judge
//! anything, and the guards it compiled in record which tests reached which
//! mutation while it does. This phase reads that run rather than making a
//! second one of its own. Two measurements of one thing are two chances to
//! disagree, and the measurement a route rests on has to be the run that
//! actually happened.

use std::collections::BTreeSet;

use rust_mutants::execute::TestTarget;
use rust_mutants::outcome::Outcome;
use rust_mutants::session::Session;

use crate::report::TargetStatus;
use crate::targets::{Target, UnitKind, WHOLE_BINARY, target_id};
use crate::trace::ProgressRecord;
use crate::ui::Notes;
use crate::watch::Watch;

/// One target, and what became of it.
#[derive(Debug, Clone)]
pub struct Measured {
    /// The target.
    pub target: Target,
    /// Its terminal state.
    pub status: TargetStatus,
    /// How long it took.
    pub duration_ms: u64,
    /// How many tests it ran, which is what that duration is the cost of.
    pub tests: u32,
    /// What it said, when that matters.
    pub message: Option<String>,
}

/// What one baseline observed.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    /// Every target, in identity order.
    pub targets: Vec<Measured>,
    /// What the compiler said, when the workspace did not build. Then there are no targets, and that is a finding rather than an error.
    pub failure: Option<String>,
    /// What this phase could not honour, by name.
    pub limitations: Vec<String>,
}

/// Where a phase says what it is doing, and what it is watched by.
#[expect(
    missing_debug_implementations,
    reason = "a stream is a handle to the outside; there is nothing to print about one"
)]
pub struct Reporting<'a, 'b> {
    /// Where progress goes.
    pub notes: &'a mut Notes<'b>,
    /// Cancellation and the trace.
    pub watch: Watch<'a>,
}

impl Reporting<'_, '_> {
    /// Says how far a phase has got, to the person watching and to the recording alike.
    ///
    /// [The trace's contract](../../../../docs/trace-v1.md) calls a `progress`
    /// event "a progress note as the UI saw it", which makes the two one
    /// statement rather than two. Saying it in one place is what keeps them
    /// one: while they were two calls the baseline phase made both and the
    /// mutation phase — the long one, the one somebody leaves running — made
    /// only the terminal's, so a recording of that run could not tell a slow
    /// phase from a stuck one.
    pub fn progress(&mut self, message: &str, done: u64, total: u64) {
        self.watch.trace.progress(ProgressRecord {
            message: message.to_owned(),
            done: Some(done),
            total: Some(total),
        });
        self.notes.progress(message, done, total);
    }
}

/// Reads what the session's one verified run of every target came to.
///
/// A target that did not pass is a row like any other. The engine hands back
/// the table whether or not anything in it passed, because the moment a reader
/// most needs the table is the moment the answer is "all of them".
#[must_use]
pub fn observe(session: &Session, mut reporting: Reporting<'_, '_>) -> Baseline {
    let watch = reporting.watch;
    let _phase = watch.trace.phase("baseline-measure");
    let verified = session.verified();
    let mut baseline = Baseline {
        limitations: limitations(session.targets(), &verified.touched.limitations),
        ..Baseline::default()
    };
    let built = |id: &str| session.targets().iter().find(|one| one.id == id);
    let rows: Vec<(&String, &rust_mutants::session::Baseline)> = verified
        .targets
        .iter()
        .filter(|(id, _observed)| !built(id).is_some_and(unmeasurable))
        .collect();
    let total = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    for (done, (id, observed)) in rows.into_iter().enumerate() {
        let built = built(id);
        let target = built.map_or_else(|| named(id), target_of);
        let done = u64::try_from(done).unwrap_or(u64::MAX).saturating_add(1);
        reporting.progress(&target.name(), done, total);
        let (status, message) = status_of(observed.outcome, observed.ignored, &observed.output);
        baseline.targets.push(Measured {
            target,
            status,
            duration_ms: u64::try_from(observed.duration.as_millis()).unwrap_or(u64::MAX),
            tests: observed.tests,
            message,
        });
    }
    baseline
}

/// The build failure a refusal to prepare carries, which is a finding rather than an error.
///
/// A workspace that does not compile is the run's answer about the workspace,
/// and a person reading a report that says so needs the compiler's first line
/// rather than an exit status. Every other refusal stays an error, because
/// every other refusal is about this run rather than about the tree.
#[must_use]
pub fn refused(error: &crate::error::RunnerError) -> Option<Baseline> {
    let crate::error::RunnerError::Engine(engine) = error else {
        return None;
    };
    let rust_mutants::error::EngineError::Session(
        rust_mutants::workspace::SessionError::PristineBroken { first },
    ) = engine
    else {
        return None;
    };
    Some(Baseline {
        failure: Some(first.clone()),
        ..Baseline::default()
    })
}

/// Whether this target answers nothing and is not a row a report carries.
///
/// One target per library is what the contract promises, and a library with
/// nothing documented is not a target that ran nothing: reporting it as
/// missing would raise a finding about documentation nobody wrote.
#[must_use]
pub fn unmeasurable(target: &TestTarget) -> bool {
    target
        .limitations
        .iter()
        .any(|name| name == rust_mutants::limitation::DOCTESTS_NONE)
}

/// Every limitation `targets` and the run's own `touched` record state, each named once.
///
/// A target that answers nothing states nothing either. A library that
/// documents no example is not a library whose examples were routed coarsely,
/// and saying both would put a reader in front of a limitation about work
/// nobody did.
#[must_use]
pub fn limitations(targets: &[TestTarget], touched: &[String]) -> Vec<String> {
    let mut named = BTreeSet::new();
    for target in targets {
        if unmeasurable(target) {
            continue;
        }
        named.extend(target.limitations.iter().cloned());
    }
    named.extend(touched.iter().cloned());
    if targets
        .iter()
        .any(|target| target.kind == rust_mutants::execute::TargetKind::ProcMacro)
    {
        let _new = named.insert(crate::limitation::PROC_MACRO_EXPANSION_NOT_MEASURED.to_owned());
    }
    named.into_iter().collect()
}

/// The runner's name for one of the engine's targets.
#[must_use]
pub fn target_of(target: &TestTarget) -> Target {
    let unit = UnitKind::of(target.kind);
    Target {
        id: target_id(&target.package, unit, &target.name, WHOLE_BINARY),
        package: target.package.clone(),
        unit,
        unit_name: target.name.clone(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: target.executable.clone(),
        cwd: target.cwd.clone(),
        env: target.cargo_env.clone(),
    }
}

/// The target one identity names, for a target the session dropped and still has a record of.
///
/// A target whose own tests do not pass is not one this run may judge against,
/// so the session does not carry it, and the report has to name it anyway: it
/// is the finding. Its identity is `package/kind/name`, which is every field a
/// row needs; the rest is how to start it, and this one is never started.
#[must_use]
pub fn named(id: &str) -> Target {
    let mut fields = id.splitn(3, '/');
    let (package, kind, name) = (
        fields.next().unwrap_or_default(),
        fields.next().unwrap_or_default(),
        fields.next().unwrap_or_default(),
    );
    let unit = UnitKind::parse(kind).unwrap_or(UnitKind::Bin);
    Target {
        id: target_id(package, unit, name, WHOLE_BINARY),
        package: package.to_owned(),
        unit,
        unit_name: name.to_owned(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: std::path::PathBuf::new(),
        cwd: std::path::PathBuf::new(),
        env: Vec::new(),
    }
}

/// What one target's verified run says became of it.
///
/// `Survived` is a target that ran and said so, whether it said it with a
/// summary line or by exiting: a binary with its own harness prints what it
/// likes and answers by its status, so counting its tests is not what decides.
///
/// `Inconclusive` is a target that executed no test, and there are two of
/// those. One whose every test libtest was told to skip has nothing to say and
/// is `Skipped`; one that ran nothing and skipped nothing is a target nothing
/// was learned about, and that is the finding. The ignored count is what tells
/// them apart, and reading the second as the first would let an absence of
/// evidence pass for a pass.
#[must_use]
pub fn status_of(outcome: Outcome, ignored: u32, output: &str) -> (TargetStatus, Option<String>) {
    match outcome {
        Outcome::Survived => (TargetStatus::Passed, None),
        Outcome::Killed => (
            TargetStatus::Failed,
            Some(failure(output).unwrap_or_else(|| "the target failed".to_owned())),
        ),
        Outcome::TimedOut => (
            TargetStatus::Failed,
            Some("the target ran out of time".to_owned()),
        ),
        Outcome::Inconclusive if ignored > 0 => (
            TargetStatus::Skipped,
            Some(format!(
                "libtest was told to skip every test of it: {ignored} ignored"
            )),
        ),
        Outcome::Inconclusive => (TargetStatus::Missing, Some(RAN_NOTHING.to_owned())),
        Outcome::NotRun => (
            TargetStatus::Missing,
            Some("the target was not run, so nothing was observed".to_owned()),
        ),
        _ => (
            TargetStatus::Missing,
            Some(failure(output).unwrap_or_else(|| {
                "the target could not be started, so nothing was observed".to_owned()
            })),
        ),
    }
}

/// The line of what a target printed that a reader would act on.
///
/// A target the engine starts prints its own failures and nothing else, and
/// the first line is the answer. One cargo runs prints a build log first, and
/// the first line of that is which crate was compiled — true, and not what
/// somebody looking at a failing test needs. So the line that names a test as
/// having failed wins, then the one the compiler or cargo marked as an error,
/// and the first line of any output at all is the last resort.
#[must_use]
pub fn failure(output: &str) -> Option<String> {
    let lines = || {
        output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
    };
    lines()
        .find(|line| line.starts_with("test ") && line.ends_with("FAILED"))
        .or_else(|| lines().find(|line| line.starts_with("error")))
        .or_else(|| lines().next())
        .map(ToOwned::to_owned)
}

/// What a target that answered and named no test of its own is recorded as having said.
pub const RAN_NOTHING: &str = "the target ran nothing, so nothing was observed";
