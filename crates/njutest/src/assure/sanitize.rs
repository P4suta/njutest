// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running the suite under a sanitizer, when the configuration asks for one.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

use super::ended::ProcessEnd;

use crate::error::RunnerError;
use crate::report::{Finding, FindingKind, Limitation};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// What a sanitizer says when it has found something.
const FOUND: [&str; 5] = [
    "ERROR: AddressSanitizer",
    "WARNING: ThreadSanitizer",
    "ERROR: LeakSanitizer",
    "MemorySanitizer:",
    "runtime error:",
];

/// What the toolchain says when it will not sanitize.
const UNAVAILABLE: [&str; 4] = [
    "only accepted on the nightly",
    "unknown `-Z` flag",
    "is not supported for this target",
    "no such command",
];

/// A sanitizer the suite can be run under, by the name `-Zsanitizer=` takes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Sanitizer {
    /// Out-of-bounds and use-after-free accesses, and the leaks it checks for too.
    Address,
    /// Memory nothing freed.
    Leak,
    /// Reads of memory nothing initialised.
    Memory,
    /// Data races.
    Thread,
}

impl Sanitizer {
    /// The name the configuration, `-Zsanitizer=` and a report spell it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Leak => "leak",
            Self::Memory => "memory",
            Self::Thread => "thread",
        }
    }
}
/// What the phase is asked to run, and how it is bounded.
#[derive(Debug, Clone)]
pub struct Sanitizing<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The cargo to drive, which must be one that understands `+nightly`.
    pub cargo: rust_mutants::cargo::Selecting<'a>,
    /// The target triple the suite is built for, which a sanitizer needs named so the host tools are not instrumented too.
    pub host: &'a str,
    /// The environment it runs with.
    pub env: rust_mutants::vars::Variables,
    /// The packages to run.
    /// Empty is the whole workspace.
    pub packages: &'a [String],
    /// The sanitizers the configuration asked for, each once.
    pub sanitizers: &'a [Sanitizer],
    /// How long one sanitizer run may take.
    pub timeout: Option<Duration>,
    /// Whether cargo may reach the network.
    pub offline: bool,
    /// Whether cargo may change the lock file.
    /// A phase that let it would measure a dependency set the baseline never saw, and would write into the tree under measurement.
    pub locked: bool,
}

/// What running the suite under the sanitizers established.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sanitized {
    /// The sanitizers the suite ran to an answer under, in the order they were asked for.
    pub ran: Vec<Sanitizer>,
    /// What they found.
    pub findings: Vec<Finding>,
    /// What they could not say.
    pub limitations: Vec<Limitation>,
}

/// Runs the suite once under each sanitizer the configuration asked for.
/// # Errors
/// Returns [`RunnerError::Interrupted`] when the run was asked to stop, and [`RunnerError::PhaseOutput`] when a sanitizer's output or its inherited flags are not valid UTF-8 and therefore cannot be interpreted exactly.
pub fn sanitize(sanitizing: &Sanitizing<'_>, watch: Watch<'_>) -> Result<Sanitized, RunnerError> {
    let mut done = Sanitized::default();
    if !sanitizing.sanitizers.is_empty() {
        done.limitations.push(Limitation::new(
            crate::limitation::Limitation::SanitizerStandardLibraryNotInstrumented,
            "the standard library the suite links is not built with the sanitizer, so what \
             it holds is not what was checked",
        ));
        for sanitizer in sanitizing.sanitizers {
            if watch.cancel.is_cancelled() {
                return Err(RunnerError::Interrupted);
            }
            one(&mut done, sanitizing, *sanitizer, watch)?;
        }
    }
    Ok(done)
}

/// Runs the suite once under one sanitizer.
fn one(
    done: &mut Sanitized,
    sanitizing: &Sanitizing<'_>,
    sanitizer: Sanitizer,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    let spec = command(sanitizing, sanitizer)?;
    let ran = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &ran));
    let failed = match ProcessEnd::of(&ran.termination) {
        ProcessEnd::Interrupted => return Err(RunnerError::Interrupted),
        ProcessEnd::Unlaunched { .. } => {
            refuse(done, sanitizer, "the toolchain would not run it");
            return Ok(());
        }
        ProcessEnd::TimedOut => {
            refuse(done, sanitizer, "it ran out of time");
            return Ok(());
        }
        ProcessEnd::Unanswered { how } => {
            refuse(
                done,
                sanitizer,
                &format!("{how}, which is no answer about the suite"),
            );
            return Ok(());
        }
        ProcessEnd::Passed => false,
        ProcessEnd::Failed => true,
    };
    let said = std::str::from_utf8(&ran.output).map_err(|source| RunnerError::PhaseOutput {
        phase: "sanitizer",
        source,
    })?;
    heard(done, sanitizer, said, failed);
    Ok(())
}

/// The command that runs the suite under `sanitizer`.
fn command(sanitizing: &Sanitizing<'_>, sanitizer: Sanitizer) -> Result<Spec, RunnerError> {
    let mut argv: Vec<OsString> = vec![
        sanitizing.cargo.path().as_os_str().to_owned(),
        OsString::from("+nightly"),
        OsString::from("test"),
        OsString::from("--target"),
        OsString::from(sanitizing.host),
    ];
    if sanitizing.packages.is_empty() {
        argv.push(OsString::from("--workspace"));
    } else {
        for package in sanitizing.packages {
            argv.push(OsString::from("--package"));
            argv.push(OsString::from(package));
        }
    }
    if sanitizing.offline {
        argv.push(OsString::from("--offline"));
    }
    if sanitizing.locked {
        argv.push(OsString::from("--locked"));
    }
    let mut spec = Spec::new(
        argv,
        sanitizing.timeout.map_or(
            rust_mutants::runner::Bound::Unbounded,
            rust_mutants::runner::Bound::After,
        ),
    );
    spec.dir = Some(sanitizing.root.to_path_buf());
    spec.env = Some(instrumenting(&sanitizing.env, sanitizer)?);
    Ok(spec)
}

/// What a run under `sanitizer` that exited by itself said, read only where the toolchain, the harness and the sanitizer's runtime speak, never inside a test's captured output.
fn heard(done: &mut Sanitized, sanitizer: Sanitizer, said: &str, failed: bool) {
    let spoken = super::deep::uncaptured(said);
    if spoken
        .iter()
        .any(|line| UNAVAILABLE.iter().any(|marker| line.contains(marker)))
    {
        refuse(done, sanitizer, "the toolchain would not run it");
        return;
    }
    done.ran.push(sanitizer);
    let name = sanitizer.name();
    if let Some(line) = FOUND.iter().find_map(|marker| {
        spoken
            .iter()
            .find(|line| line.contains(marker))
            .map(|line| line.trim().to_owned())
    }) {
        done.findings.push(Finding {
            kind: FindingKind::UndefinedBehaviour,
            subject: format!("sanitizer:{name}"),
            detail: line,
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
    } else if failed {
        done.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: format!("sanitizer:{name}"),
            detail: format!("a test fails under {name} that passes without it"),
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
    }
}

/// A sanitizer that was asked for and could not be run to an answer: a gap somebody asked to close, stated as one.
fn refuse(done: &mut Sanitized, sanitizer: Sanitizer, why: &str) {
    let name = sanitizer.name();
    done.limitations.push(Limitation::new(
        crate::limitation::Limitation::SanitizerUnavailable,
        &format!("{name} was asked for and {why}"),
    ));
    done.findings.push(Finding {
        kind: FindingKind::NotMeasured,
        subject: format!("sanitizer:{name}"),
        detail: format!("the suite was not run under {name}, which the configuration asks for"),
        origin: crate::report::FindingOrigin::Global,
        path: None,
        position: None,
    });
}

/// The environment one sanitizer run adds: the flag, and nothing else the caller did not already have.
fn instrumenting(
    base: &rust_mutants::vars::Variables,
    sanitizer: Sanitizer,
) -> Result<rust_mutants::vars::Variables, RunnerError> {
    let mut env = base.clone();
    let mut flags: Vec<String> = match env.var("RUSTFLAGS") {
        Some(value) => std::str::from_utf8(value.as_encoded_bytes())
            .map_err(|source| RunnerError::PhaseOutput {
                phase: "sanitizer RUSTFLAGS",
                source,
            })?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        None => Vec::new(),
    };
    env.remove("CARGO_ENCODED_RUSTFLAGS");
    flags.push(format!("-Zsanitizer={}", sanitizer.name()));
    env.set("RUSTFLAGS", flags.join(" "));
    Ok(env)
}
