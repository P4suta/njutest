// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving the fuzz targets a tree holds, and what their crashes become.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

use crate::error::RunnerError;
use crate::report::{Finding, FindingKind, Limitation};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// Where cargo-fuzz keeps the targets.
pub const TARGETS: &str = "fuzz/fuzz_targets";

/// Where it puts what crashed them.
pub const ARTIFACTS: &str = "fuzz/artifacts";

/// Where an input that is worth keeping goes.
pub const CORPUS: &str = "fuzz/corpus";

/// What the toolchain says when it has no cargo-fuzz.
const ABSENT: [&str; 3] = [
    "no such command",
    "no such subcommand",
    "is not installed for the toolchain",
];

/// One input that crashed a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    /// The target it crashed.
    pub target: String,
    /// Where cargo-fuzz wrote it, workspace-relative.
    pub artifact: String,
    /// Where it would go in the corpus, workspace-relative.
    pub corpus: String,
    /// The input itself.
    pub content: Vec<u8>,
}

/// What driving the targets established.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fuzzed {
    /// The targets that were driven, in name order.
    pub ran: Vec<String>,
    /// Every input that crashed one.
    pub crashes: Vec<Crash>,
    /// What the run found wrong.
    pub findings: Vec<Finding>,
    /// What it could not say.
    pub limitations: Vec<Limitation>,
}

/// What the phase drives, and how it is bounded.
#[derive(Debug, Clone)]
pub struct Fuzzing<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The cargo to drive, which must be one that understands `+nightly`.
    pub cargo: &'a Path,
    /// The environment it runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The targets to drive. Empty is every target the tree holds.
    pub targets: &'a [String],
    /// How long one target is driven for.
    pub max_total_time: Duration,
    /// How long the whole command may take, which must be longer than that.
    pub timeout: Option<Duration>,
}

/// The fuzz targets a tree holds, by name, in name order.
///
/// # Errors
/// Returns the filesystem error that prevents a complete enumeration.
pub fn targets_of(root: &Path) -> io::Result<Vec<String>> {
    let entries = match std::fs::read_dir(root.join(TARGETS)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|kind| kind == "rs") {
            let stem = path.file_stem().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} has an `.rs` extension but no file stem", path.display()),
                )
            })?;
            let name = stem.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not valid UTF-8", path.display()),
                )
            })?;
            found.push(name.to_owned());
        }
    }
    found.sort();
    Ok(found)
}

/// Says a tree holds targets nobody asked to drive.
#[must_use]
pub fn found(targets: &[String]) -> Limitation {
    Limitation::new(
        crate::limitation::FUZZ_NOT_EXECUTED,
        &format!(
            "{} fuzz targets are here and were not driven, so nothing is claimed about what \
             they would find; `[fuzz] run = true` drives them: {}",
            targets.len(),
            targets.join(", ")
        ),
    )
}

/// Drives every selected target once.
/// # Errors
/// Returns [`RunnerError::PhaseOutput`] when cargo-fuzz violates its text
/// output contract; its result cannot then be classified exactly.
pub fn fuzz(fuzzing: &Fuzzing<'_>, watch: Watch<'_>) -> Result<Fuzzed, RunnerError> {
    let mut done = Fuzzed::default();
    let held = match targets_of(fuzzing.root) {
        Ok(held) => held,
        Err(error) => {
            unavailable(&mut done, "targets", &error);
            return Ok(done);
        }
    };
    let selected: Vec<String> = if fuzzing.targets.is_empty() {
        held
    } else {
        held.into_iter()
            .filter(|target| fuzzing.targets.contains(target))
            .collect()
    };
    for target in &selected {
        if watch.cancel.is_cancelled() {
            break;
        }
        one(&mut done, fuzzing, target, watch)?;
    }
    Ok(done)
}

/// Drives one target and reads what it left behind.
fn one(
    done: &mut Fuzzed,
    fuzzing: &Fuzzing<'_>,
    target: &str,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    let before = match artifacts(fuzzing.root, target) {
        Ok(before) => before,
        Err(error) => {
            unavailable(done, target, &error);
            return Ok(());
        }
    };
    let mut spec = fuzz_spec(fuzzing, target);
    spec.dir = Some(fuzzing.root.to_path_buf());
    spec.env = Some(fuzzing.env.clone());

    let ran = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &ran));
    let said = std::str::from_utf8(&ran.output).map_err(|source| RunnerError::PhaseOutput {
        phase: "cargo-fuzz",
        source,
    })?;
    let after = match artifacts(fuzzing.root, target) {
        Ok(after) => after,
        Err(error) => {
            unavailable(done, target, &error);
            return Ok(());
        }
    };
    let left: Vec<Artifact> = after
        .into_iter()
        .filter(|artifact| !before.contains(artifact))
        .collect();
    let undriven = ran.error().is_some()
        || ABSENT.iter().any(|marker| said.contains(marker))
        || (!ran.timed_out() && ran.conventional_exit_code() != 0 && left.is_empty());
    if undriven {
        done.limitations.push(Limitation::new(
            crate::limitation::CARGO_FUZZ_UNAVAILABLE,
            &format!("{target} was to be driven and cargo-fuzz could not be run"),
        ));
        done.findings.push(Finding {
            kind: FindingKind::NotMeasured,
            subject: format!("fuzz:{target}"),
            detail: format!("{target} was not driven, which the configuration asks for"),
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
        return Ok(());
    }
    done.ran.push(target.to_owned());
    if ran.timed_out() {
        done.limitations.push(Limitation::new(
            crate::limitation::CARGO_FUZZ_UNAVAILABLE,
            &format!("{target} ran out of time before it was driven for as long as it was asked"),
        ));
    }
    record_crashes(done, target, left);
    Ok(())
}

fn fuzz_spec(fuzzing: &Fuzzing<'_>, target: &str) -> Spec {
    Spec::new(
        [
            fuzzing.cargo.as_os_str().to_owned(),
            OsString::from("+nightly"),
            OsString::from("fuzz"),
            OsString::from("run"),
            OsString::from(target),
            OsString::from("--"),
            OsString::from(format!(
                "-max_total_time={}",
                fuzzing.max_total_time.as_secs()
            )),
        ],
        match fuzzing.timeout {
            Some(timeout) => rust_mutants::runner::Bound::After(timeout),
            None => rust_mutants::runner::Bound::Unbounded,
        },
    )
}

fn record_crashes(done: &mut Fuzzed, target: &str, left: Vec<Artifact>) {
    for artifact in left {
        let content = match std::fs::read(&artifact.on_disk) {
            Ok(content) => content,
            Err(error) => {
                unavailable(done, target, &error);
                continue;
            }
        };
        let name = match artifact.reported.rsplit('/').next() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => artifact.reported.clone(),
        };
        done.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: format!("fuzz:{target}"),
            detail: format!("{target} crashed on the input {} kept", artifact.reported),
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
        done.crashes.push(Crash {
            target: target.to_owned(),
            artifact: artifact.reported,
            corpus: format!("{CORPUS}/{target}/{name}"),
            content,
        });
    }
}

fn unavailable(done: &mut Fuzzed, subject: &str, error: &io::Error) {
    done.limitations.push(Limitation::new(
        crate::limitation::CARGO_FUZZ_UNAVAILABLE,
        &format!("fuzz:{subject} could not be read completely: {error}"),
    ));
    done.findings.push(Finding {
        kind: FindingKind::NotMeasured,
        subject: format!("fuzz:{subject}"),
        detail: format!("filesystem traversal failed: {error}"),
        origin: crate::report::FindingOrigin::Global,
        path: None,
        position: None,
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Artifact {
    on_disk: PathBuf,
    reported: String,
}

/// What is in one target's artifact directory, workspace-relative, in name order.
fn artifacts(root: &Path, target: &str) -> io::Result<Vec<Artifact>> {
    let directory = PathBuf::from(ARTIFACTS).join(target);
    let entries = match std::fs::read_dir(root.join(&directory)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let on_disk = entry.path();
            let reported = rust_mutants::id::slashed(&directory.join(entry.file_name()))
                .map_err(io::Error::other)?;
            found.push(Artifact { on_disk, reported });
        }
    }
    found.sort_by(|one, other| one.reported.cmp(&other.reported));
    Ok(found)
}
