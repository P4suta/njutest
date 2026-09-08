// SPDX-FileCopyrightText: 2026 mjutest contributors
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

/// Reads what the session's one verified run of every target came to.
///
/// A target that did not pass is a row like any other. The engine hands back
/// the table whether or not anything in it passed, because the moment a reader
/// most needs the table is the moment the answer is "all of them".
#[must_use]
pub fn observe(session: &Session, reporting: Reporting<'_, '_>) -> Baseline {
    let Reporting { notes, watch } = reporting;
    let phase = watch.trace.phase("baseline-measure");
    let verified = session.verified();
    let mut baseline = Baseline {
        limitations: limitations(session),
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
        watch.trace.progress(ProgressRecord {
            message: target.name(),
            done: Some(done),
            total: Some(total),
        });
        notes.progress(&target.name(), done, total);
        let (status, message) = status_of(observed.outcome, &observed.output);
        baseline.targets.push(Measured {
            target,
            status,
            duration_ms: u64::try_from(observed.duration.as_millis()).unwrap_or(u64::MAX),
            tests: observed.tests,
            message,
        });
    }
    phase.end();
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
fn unmeasurable(target: &TestTarget) -> bool {
    target
        .limitations
        .iter()
        .any(|name| name == rust_mutants::limitation::DOCTESTS_NONE)
}

/// Every limitation this run's targets and their records state, each named once.
///
/// A target that answers nothing states nothing either. A library that
/// documents no example is not a library whose examples were routed coarsely,
/// and saying both would put a reader in front of a limitation about work
/// nobody did.
fn limitations(session: &Session) -> Vec<String> {
    let mut named = BTreeSet::new();
    for target in session.targets() {
        if unmeasurable(target) {
            continue;
        }
        named.extend(target.limitations.iter().cloned());
    }
    named.extend(session.verified().touched.limitations.iter().cloned());
    if session
        .targets()
        .iter()
        .any(|target| target.kind == rust_mutants::execute::TargetKind::ProcMacro)
    {
        let _new = named.insert(PROC_MACRO_LIMITATION.to_owned());
    }
    named.into_iter().collect()
}

/// The runner's name for one of the engine's targets.
fn target_of(target: &TestTarget) -> Target {
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
fn named(id: &str) -> Target {
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
/// `Inconclusive` is a target that ran and executed no test — a harness that
/// printed no summary, a library with nothing documented, or one whose every
/// test is `#[ignore]`d. The first two are findings and the third is not, and
/// what tells them apart is the ignored count, which does not cross this
/// boundary yet. Until it does they are all the finding: an absence of
/// evidence that reads as a pass is the one direction this runner does not
/// take.
#[must_use]
pub fn status_of(outcome: Outcome, output: &str) -> (TargetStatus, Option<String>) {
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
fn failure(output: &str) -> Option<String> {
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

/// The name a run states when a library's documentation was run: rustdoc compiles each example into a binary this run never sees, so the coverage it carries is the whole of every file its library is made of.
pub const DOCTESTS_LIMITATION: &str = rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE;

/// The name a run states when a test binary brings its own harness, which makes the whole binary one target rather than one target per test.
pub const WHOLE_BINARY_LIMITATION: &str = rust_mutants::limitation::CUSTOM_HARNESS;

/// The name a run states when a procedural macro is in scope: what the macro expands to is decided during the build, and this run does not measure it.
pub const PROC_MACRO_LIMITATION: &str = "proc-macro-expansion-not-measured";

/// The name a run states when a target's own tests do not pass, so no outcome against it would be about a mutation.
pub const NOT_PASSING_LIMITATION: &str = rust_mutants::limitation::BASELINE_NOT_PASSING;

/// The name a run states when a target's guards recorded nothing, so every test of it reaches every mutation in it.
pub const UNRECORDED_LIMITATION: &str = rust_mutants::limitation::TOUCH_NOT_RECORDED;
