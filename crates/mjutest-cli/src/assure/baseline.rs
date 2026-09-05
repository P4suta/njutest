// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: every test of the workspace, once, in its own process, under coverage instrumentation.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use rust_mutants::cargo::{Package, Toolchain};
use rust_mutants::execute::{Summary, parse_summary};
use rust_mutants::runner::{Spec, run as run_process};

use crate::build::{self, BuildOptions, Cargo, Flavour, Selection};
use crate::coverage::{Block, Tools, covered, instrumented, profile_pattern, written_profiles};
use crate::error::RunnerError;
use crate::report::TargetStatus;
use crate::targets::{Target, enumerate};
use crate::trace::{ExecRecord, ProgressRecord};
use crate::ui::Notes;
use crate::watch::Watch;

/// The compiled workspace a phase works against: one argument, so a phase that also takes options, notes, and a watch still reads.
#[derive(Debug, Clone, Copy)]
pub struct Workspace<'a> {
    /// The located toolchain.
    pub toolchain: &'a Toolchain,
    /// What `cargo metadata` said the workspace holds.
    pub packages: &'a [Package],
}

/// What to measure, and where to put what measuring produces.
#[derive(Debug, Clone)]
pub struct BaselineOptions {
    /// The workspace root.
    pub root: PathBuf,
    /// What to compile.
    pub selection: Selection,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// The environment every command and test process runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The instrumented build's layer.
    pub target_dir: PathBuf,
    /// The scratch layer anything a test starts writes into.
    pub scratch_build_dir: PathBuf,
    /// Where test processes write their coverage profiles.
    pub profiles_dir: PathBuf,
    /// How long one target may take.
    pub timeout: Option<Duration>,
    /// Arguments for the test binaries, after `--`.
    pub test_args: Vec<String>,
}

/// One target, and what became of it.
#[derive(Debug, Clone)]
pub struct Measured {
    /// The target.
    pub target: Target,
    /// Its terminal state.
    pub status: TargetStatus,
    /// How long it took.
    pub duration_ms: u64,
    /// What it said, when that matters.
    pub message: Option<String>,
    /// The regions it reached. Empty for a target that did not run.
    pub covered: BTreeSet<Block>,
}

/// What one baseline observed.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    /// Every target, in the order the build produced them.
    pub targets: Vec<Measured>,
    /// Every region the build instrumented, whether or not anything reached it: the denominator routing is defined against.
    pub instrumented: BTreeSet<Block>,
    /// What the compiler said, when the workspace did not build. Then there are no targets, and that is a finding rather than an error.
    pub failure: Option<String>,
    /// What this phase could not honour, by name.
    pub limitations: Vec<String>,
}

/// Builds the workspace instrumented, then runs each of its tests once.
///
/// # Errors
/// The build's refusals, a test binary that could not be asked what it
/// holds, and a coverage tool that failed. A test that fails is not an
/// error: it is a [`Measured`] that failed.
pub fn run(
    workspace: Workspace<'_>,
    options: &BaselineOptions,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<Baseline, RunnerError> {
    let phase = watch.trace.phase("baseline");
    let built = build::build(
        workspace.toolchain,
        workspace.packages,
        &BuildOptions {
            root: options.root.clone(),
            selection: options.selection.clone(),
            flavour: Flavour::Coverage,
            target_dir: options.target_dir.clone(),
            scratch_build_dir: options.scratch_build_dir.clone(),
            env: options.env.clone(),
            cargo: options.cargo,
            timeout: options.timeout,
        },
        watch,
    )?;
    if let Some(failure) = built.failure {
        phase.end();
        return Ok(Baseline {
            failure: Some(failure),
            limitations: built.limitations,
            ..Baseline::default()
        });
    }

    let tools = Tools::locate(workspace.toolchain, &options.root, watch)?;
    let mut baseline = Baseline {
        limitations: built.limitations,
        ..Baseline::default()
    };
    let mut selected = Vec::new();
    for unit in &built.units {
        selected.extend(enumerate(unit, watch)?);
    }
    let total = u64::try_from(selected.len()).unwrap_or(u64::MAX);
    for (index, target) in selected.into_iter().enumerate() {
        let done = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        watch.trace.progress(ProgressRecord {
            message: target.name(),
            done: Some(done),
            total: Some(total),
        });
        notes.progress(&target.name(), done, total);
        let (measured, seen) = measure(&tools, &target, options, watch)?;
        baseline.instrumented.extend(seen);
        baseline.targets.push(measured);
    }
    phase.end();
    Ok(baseline)
}

/// Runs one target and reads what it reached, together with every region the export said the build instrumented — which is a fact about the binary rather than about this target, and the caller unions.
fn measure(
    tools: &Tools,
    target: &Target,
    options: &BaselineOptions,
    watch: Watch<'_>,
) -> Result<(Measured, BTreeSet<Block>), RunnerError> {
    let spec = command(target, options);
    let ran = run_process(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &ran));
    let (status, message) = status_of(parse_summary(&ran.output), ran.timed_out);
    let duration_ms = u64::try_from(ran.duration.as_millis()).unwrap_or(u64::MAX);

    let mut reached = BTreeSet::new();
    let mut seen = BTreeSet::new();
    if status != TargetStatus::Missing {
        let profiles = written_profiles(&options.profiles_dir, &target.id)?;
        if !profiles.is_empty() {
            let merged = options.profiles_dir.join(format!("{}.profdata", target.id));
            tools.merge(&profiles, &merged, watch)?;
            let files = tools.export(&merged, &target.executable, watch)?;
            reached = covered(&files);
            seen = instrumented(&files);
        }
    }
    Ok((
        Measured {
            target: target.clone(),
            status,
            duration_ms,
            message,
            covered: reached,
        },
        seen,
    ))
}

/// The command one target runs as: exactly that test, in terse form, with whatever the caller asked the binaries for.
fn command(target: &Target, options: &BaselineOptions) -> Spec {
    let mut argv: Vec<OsString> = vec![target.executable.as_os_str().to_owned()];
    if !target.is_whole_binary() {
        argv.push(OsString::from(&target.path));
        argv.push(OsString::from("--exact"));
    }
    argv.push(OsString::from("--format"));
    argv.push(OsString::from("terse"));
    argv.extend(options.test_args.iter().map(OsString::from));

    let mut spec = Spec::new(argv);
    spec.dir = Some(target.cwd.clone());
    let mut env = target.env.clone();
    env.retain(|(key, _)| key != OsStr::new("LLVM_PROFILE_FILE"));
    env.push((
        OsString::from("LLVM_PROFILE_FILE"),
        profile_pattern(&options.profiles_dir, &target.id).into_os_string(),
    ));
    spec.env = Some(env);
    spec.timeout = options.timeout;
    spec
}

/// What the summary line says became of a target.
#[must_use]
pub fn status_of(summary: Option<Summary>, timed_out: bool) -> (TargetStatus, Option<String>) {
    if timed_out {
        return (
            TargetStatus::Failed,
            Some("the target ran out of time".to_owned()),
        );
    }
    let Some(summary) = summary else {
        return (
            TargetStatus::Missing,
            Some("the target printed no result line, so nothing was observed".to_owned()),
        );
    };
    if summary.failed > 0 {
        return (
            TargetStatus::Failed,
            Some(format!("{} failed", summary.failed)),
        );
    }
    if summary.passed > 0 {
        return (TargetStatus::Passed, None);
    }
    if summary.ignored > 0 {
        return (TargetStatus::Skipped, Some("libtest ignored it".to_owned()));
    }
    (
        TargetStatus::Missing,
        Some("the target ran nothing, so nothing was observed".to_owned()),
    )
}
