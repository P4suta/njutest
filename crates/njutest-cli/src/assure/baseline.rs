// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: what the one verified run of every target observed.

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
    #[cfg(any(test, feature = "testkit"))]
    #[cfg(feature = "testkit")]
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
#[derive(Debug)]
pub struct Reporting<'a, 'b> {
    /// Where progress goes.
    pub notes: &'a mut Notes<'b>,
    /// Cancellation and the trace.
    pub watch: Watch<'a>,
}

/// One step of a phase, in the two forms its two readers want.
///
/// A person watching reads `said`; an audit follows `subject` back to the one
/// thing the step was about. Held together so a caller cannot give one and
/// forget the other.
#[derive(Debug, Clone, Copy)]
pub struct Step<'a> {
    /// What a person watching reads.
    pub said: &'a str,
    /// What a later command takes, where the step is about something that has a name.
    pub subject: &'a str,
}

impl<'a> Step<'a> {
    /// A step about nothing a later command can be given.
    #[must_use]
    pub const fn of(said: &'a str) -> Self {
        Self { said, subject: "" }
    }

    /// A step about `subject`.
    #[must_use]
    pub const fn about(said: &'a str, subject: &'a str) -> Self {
        Self { said, subject }
    }
}

impl Reporting<'_, '_> {
    /// Says how far a phase has got, to the person watching and to the recording alike.
    ///
    /// # Errors
    /// Returns the progress stream's write failure.
    pub fn progress(&mut self, message: &str, done: u64, total: u64) -> std::io::Result<()> {
        self.about(Step::of(message), done, total)
    }

    /// The same, where the step is about something a later command can be given.
    ///
    /// # Errors
    /// Returns the progress stream's write failure.
    pub fn about(&mut self, step: Step<'_>, done: u64, total: u64) -> std::io::Result<()> {
        self.watch.trace.progress(ProgressRecord {
            message: step.said.to_owned(),
            subject: step.subject.to_owned(),
            done: Some(done),
            total: Some(total),
        });
        self.notes.progress(step.said, done, total)
    }
}

/// Reads what the session's one verified run of every target came to.
/// # Errors
/// Returns a typed target refusal if an engine target name cannot be framed by
/// the runner's stable identity recipe.
pub fn observe(
    session: &Session,
    mut reporting: Reporting<'_, '_>,
) -> Result<Baseline, crate::error::RunnerError> {
    let watch = reporting.watch;
    let phase = watch.trace.phase("baseline-measure");
    let verified = session.verified();
    let mut baseline = Baseline {
        limitations: limitations(session.targets(), &verified.touched.limitations),
        ..Baseline::default()
    };
    let built = |id: &str| session.targets().iter().find(|one| one.id.as_str() == id);
    let rows: Vec<(&String, &rust_mutants::session::Baseline)> = verified
        .targets
        .iter()
        .filter(|(id, _observed)| !built(id).is_some_and(unmeasurable))
        .map(|(id, measured)| (id, measured.baseline()))
        .collect();
    let total = u64::try_from(rows.len())
        .map_err(|error| crate::targets::TargetError::invalid("baseline target count", error))?;
    for (done, (id, observed)) in rows.into_iter().enumerate() {
        let built = built(id);
        let target = match built {
            Some(target) => target_of(target)?,
            None => named(id)?,
        };
        let done = u64::try_from(done)
            .map_err(|error| crate::targets::TargetError::invalid("baseline progress", error))?
            .checked_add(1)
            .ok_or_else(|| {
                crate::targets::TargetError::invalid(
                    "baseline progress",
                    "the progress count overflowed",
                )
            })?;
        reporting.progress(&target.name(), done, total)?;
        let (status, message) = status_of(observed.outcome, observed.ignored, &observed.output);
        let duration_ms = u64::try_from(observed.duration.as_millis())
            .map_err(|error| crate::targets::TargetError::invalid(target.name(), error))?;
        baseline.targets.push(Measured {
            target,
            status,
            duration_ms,
            #[cfg(any(test, feature = "testkit"))]
            #[cfg(feature = "testkit")]
            tests: observed.tests,
            message,
        });
    }
    drop(phase);
    Ok(baseline)
}

/// The build failure a refusal to prepare carries, which is a finding rather than an error.
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
#[must_use]
pub fn unmeasurable(target: &TestTarget) -> bool {
    target
        .limitations
        .iter()
        .any(|name| name == rust_mutants::limitation::DOCTESTS_NONE)
}

/// Every limitation `targets` and the run's own `touched` record state, each named once.
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
        named.extend(std::iter::once(
            crate::limitation::PROC_MACRO_EXPANSION_NOT_MEASURED.to_owned(),
        ));
    }
    named.into_iter().collect()
}

/// The runner's name for one of the engine's targets.
/// # Errors
/// Returns a typed refusal if a target field cannot be framed by the stable
/// identity recipe.
pub fn target_of(target: &TestTarget) -> Result<Target, crate::targets::TargetError> {
    let unit = UnitKind::of(target.kind);
    Ok(Target {
        id: target_id(&target.package, unit, &target.name, WHOLE_BINARY)
            .map_err(|error| crate::targets::TargetError::invalid(&target.name, error))?,
        package: target.package.clone(),
        unit,
        unit_name: target.name.clone(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: target.executable.clone(),
        cwd: target.cwd.clone(),
        env: target.cargo_env.clone(),
    })
}

/// The target one identity names, for a target the session dropped and still has a record of.
/// # Errors
/// Returns a typed refusal when an engine target name is not the exact
/// `package/kind/name` spelling or cannot be framed as a target identity.
pub fn named(id: &str) -> Result<Target, crate::targets::TargetError> {
    let mut fields = id.splitn(3, '/');
    let (Some(package), Some(kind), Some(name)) = (fields.next(), fields.next(), fields.next())
    else {
        return Err(crate::targets::TargetError::invalid(
            id,
            "engine target identity is not package/kind/name",
        ));
    };
    if package.is_empty() || name.is_empty() {
        return Err(crate::targets::TargetError::invalid(
            id,
            "engine target identity has an empty package or target name",
        ));
    }
    let Some(unit) = UnitKind::parse(kind) else {
        return Err(crate::targets::TargetError::invalid(
            id,
            "engine target identity has an unknown unit kind",
        ));
    };
    Ok(Target {
        id: target_id(package, unit, name, WHOLE_BINARY)
            .map_err(|error| crate::targets::TargetError::invalid(id, error))?,
        package: package.to_owned(),
        unit,
        unit_name: name.to_owned(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: std::path::PathBuf::new(),
        cwd: std::path::PathBuf::new(),
        env: Vec::new(),
    })
}

/// What one target's verified run says became of it.
#[must_use]
pub fn status_of(outcome: Outcome, ignored: u32, output: &str) -> (TargetStatus, Option<String>) {
    match outcome {
        Outcome::Survived => (TargetStatus::Passed, None),
        Outcome::Killed => (
            TargetStatus::Failed,
            Some(failure(output).unwrap_or_else(|| "the target failed".to_owned())),
        ),
        Outcome::StepLimitReached => (
            TargetStatus::Failed,
            Some("the target reached the run's guard-take allowance".to_owned()),
        ),
        Outcome::Waited => (
            TargetStatus::Failed,
            Some("this machine stopped waiting for the target".to_owned()),
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
        Outcome::Errored => (
            TargetStatus::Missing,
            Some(failure(output).unwrap_or_else(|| {
                "the target could not be started, so nothing was observed".to_owned()
            })),
        ),
    }
}

/// The line of what a target printed that a reader would act on.
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
