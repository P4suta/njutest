// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving the fuzz targets a tree holds, and what their crashes become.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

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
#[must_use]
pub fn targets_of(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(TARGETS)) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "rs"))
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    found.sort();
    found
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
#[must_use]
pub fn fuzz(fuzzing: &Fuzzing<'_>, watch: Watch<'_>) -> Fuzzed {
    let mut done = Fuzzed::default();
    let held = targets_of(fuzzing.root);
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
        one(&mut done, fuzzing, target, watch);
    }
    done
}

/// Drives one target and reads what it left behind.
fn one(done: &mut Fuzzed, fuzzing: &Fuzzing<'_>, target: &str, watch: Watch<'_>) {
    let before = artifacts(fuzzing.root, target);
    let mut spec = Spec::new(
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
        fuzzing.timeout.map_or(
            rust_mutants::runner::Bound::Unbounded,
            rust_mutants::runner::Bound::After,
        ),
    );
    spec.dir = Some(fuzzing.root.to_path_buf());
    spec.env = Some(fuzzing.env.clone());

    let ran = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &ran));
    let said = String::from_utf8_lossy(&ran.output).into_owned();
    let left: Vec<Artifact> = artifacts(fuzzing.root, target)
        .into_iter()
        .filter(|artifact| !before.contains(artifact))
        .collect();
    let undriven = ran.error.is_some()
        || ABSENT.iter().any(|marker| said.contains(marker))
        || (!ran.timed_out && ran.exit_code != 0 && left.is_empty());
    if undriven {
        done.limitations.push(Limitation::new(
            crate::limitation::CARGO_FUZZ_UNAVAILABLE,
            &format!("{target} was to be driven and cargo-fuzz could not be run"),
        ));
        done.findings.push(Finding {
            kind: FindingKind::NotMeasured,
            subject: format!("fuzz:{target}"),
            detail: format!("{target} was not driven, which the configuration asks for"),
            path: None,
            position: None,
        });
        return;
    }
    done.ran.push(target.to_owned());
    if ran.timed_out {
        done.limitations.push(Limitation::new(
            crate::limitation::CARGO_FUZZ_UNAVAILABLE,
            &format!("{target} ran out of time before it was driven for as long as it was asked"),
        ));
    }
    for artifact in left {
        let Ok(content) = std::fs::read(&artifact.on_disk) else {
            continue;
        };
        let name = Path::new(&artifact.reported).file_name().map_or_else(
            || artifact.reported.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        done.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: format!("fuzz:{target}"),
            detail: format!("{target} crashed on the input {} kept", artifact.reported),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Artifact {
    on_disk: PathBuf,
    reported: String,
}

/// What is in one target's artifact directory, workspace-relative, in name order.
fn artifacts(root: &Path, target: &str) -> Vec<Artifact> {
    let directory = PathBuf::from(ARTIFACTS).join(target);
    let Ok(entries) = std::fs::read_dir(root.join(&directory)) else {
        return Vec::new();
    };
    let mut found: Vec<Artifact> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| Artifact {
            on_disk: entry.path(),
            reported: directory
                .join(entry.file_name())
                .to_string_lossy()
                .replace('\\', "/"),
        })
        .collect();
    found.sort_by(|one, other| one.reported.cmp(&other.reported));
    found
}
