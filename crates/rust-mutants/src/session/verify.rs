// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one run of every target with nothing active: the baseline check, and the measurement that rides on it.
//!
//! Every guard of the instrumented tree runs on it, and libtest names each
//! test's thread after the test, so asking the guards what they reached costs
//! the run nothing it was not already spending. What they say is read here,
//! and a target this run cannot ask is named rather than read as having
//! reached nothing.

use std::collections::BTreeMap;
use std::time::Duration;

use super::prepare::Building;
use crate::EngineError;
use crate::catalog::Catalog;
use crate::execute::{self, Context, ExecRequest, TargetKind, TestTarget};
use crate::workspace::{SessionError, Workspace};

/// Runs every target once with nothing active. A tree whose instrumented baseline fails is one whose every later result would be about the instrumentation rather than about a mutant.
pub(super) fn verify(
    workspace: &Workspace,
    targets: &mut [TestTarget],
    scratch: &std::path::Path,
    building: &Building<'_>,
) -> Result<Verified, EngineError> {
    let Building {
        cancel,
        catalog,
        asked,
        ..
    } = *building;
    let phase = workspace.trace.phase("verify");
    let logs = scratch.join("touch");
    std::fs::create_dir_all(&logs).map_err(|source| SessionError::WriteFailed {
        path: logs.display().to_string(),
        source,
    })?;
    let mut verified = Verified::default();
    for target in targets.iter_mut() {
        let recording =
            (asked && recordable(target)).then(|| logs.join(format!("{}.log", slug(&target.id))));
        if let Some(path) = &recording {
            drop(std::fs::remove_file(path));
        }
        let context = Context {
            base_env: &workspace.base_env,
            cargo: Some(workspace.toolchain.cargo()),
            sysroot: workspace.toolchain.sysroot(),
            active: None,
            probe: None,
            touch: recording.as_deref(),
            profile: None,
        };
        let request = ExecRequest::new(target).with_scratch(scratch);
        let result = execute::exec(&request, &context, cancel, &workspace.trace);
        workspace.trace.verify(crate::trace::VerifyRecord {
            target: target.id.clone(),
            outcome: result.outcome.name().to_owned(),
            tests_run: result.tests_run,
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
        });
        let _kept = verified.baseline.insert(target.id.clone(), result.duration);
        let _counted = verified.ran.insert(
            target.id.clone(),
            result
                .tests_run
                .unwrap_or_else(|| u32::try_from(result.passed_tests.len()).unwrap_or(u32::MAX)),
        );
        if target.kind == TargetKind::Doc && result.tests_run == Some(0) {
            target
                .limitations
                .push(crate::limitation::DOCTESTS_NONE.to_owned());
        }
        if result.exit_code == crate::instrument::TOUCH_UNAVAILABLE_EXIT {
            verified
                .touched
                .limited(crate::touch::UNRECORDED, &target.id);
            return Err(EngineError::from(SessionError::VerifyFailed {
                target: target.id.clone(),
                output: String::from_utf8_lossy(&result.output).into_owned(),
            }));
        }
        if !matches!(
            result.outcome,
            crate::outcome::Outcome::Survived | crate::outcome::Outcome::Inconclusive
        ) {
            return Err(EngineError::from(SessionError::VerifyFailed {
                target: target.id.clone(),
                output: String::from_utf8_lossy(&result.output).into_owned(),
            }));
        }
        gather(
            &mut verified.touched,
            &Recording {
                target: &target.id,
                log: recording.as_deref(),
                catalog,
                ran: &result.passed_tests,
            },
            &workspace.trace,
        );
    }
    phase.end();
    Ok(verified)
}

/// What the one run of every target with nothing activated established.
#[derive(Debug, Default)]
pub(super) struct Verified {
    /// How long each target's own baseline took, which is what a derived timeout is a multiple of.
    pub(super) baseline: BTreeMap<String, Duration>,
    /// How many tests each target's baseline ran, which is what asking the whole of it about one mutation costs.
    pub(super) ran: BTreeMap<String, u32>,
    /// What the guards recorded on that same run.
    pub(super) touched: crate::touch::Touched,
}

/// One target's record, and what makes sense of it.
struct Recording<'a> {
    /// The target the record is about.
    target: &'a str,
    /// Where its guards were told to append, or nothing when they were not asked.
    log: Option<&'a std::path::Path>,
    /// The catalog the record must be about.
    catalog: &'a Catalog,
    /// Every test the run of it passed, which is what names a thread a touch can be attributed to.
    ran: &'a [String],
}

/// Whether a target's guards can be asked what they reached.
///
/// A target the engine starts itself gets the variable and the process that
/// reads it. One started through something else — a documented example, which
/// rustdoc compiles and runs, or a runner the project configured — is one this
/// engine cannot promise the variable reaches, so it is not asked rather than
/// read as having reached nothing.
fn recordable(target: &TestTarget) -> bool {
    target.kind != TargetKind::Doc && target.through.is_empty()
}

/// Reads one target's record into `touched`, or says why there is nothing of it to read.
///
/// A target that was asked and wrote nothing reached nothing: the runtime
/// appends the first time any guard of the process runs, so an absent file is
/// a process whose guards never ran rather than a process that was never
/// asked. A record naming a thread the run does not know as one of its tests
/// is a touch nothing can be attributed to, and reaches every test of the
/// target.
fn gather(
    touched: &mut crate::touch::Touched,
    recording: &Recording<'_>,
    trace: &crate::trace::Recorder,
) {
    let Some(log) = recording.log else {
        touched.limited(crate::touch::UNRECORDED, recording.target);
        return;
    };
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let count = u32::try_from(recording.catalog.mutants().len()).unwrap_or(u32::MAX);
    let recorded = match crate::touch::read(&text, recording.catalog.digest(), count) {
        Ok(recorded) => recorded,
        Err(error) => {
            trace.note(
                crate::touch::UNREADABLE,
                &format!("{}: {error}", recording.target),
            );
            touched.limited(crate::touch::UNREADABLE, recording.target);
            return;
        }
    };
    let mut gathered = crate::touch::TargetTouches {
        loose: recorded.loose,
        loose_bodies: recorded.loose_bodies,
        ran: recording.ran.to_vec(),
        ..crate::touch::TargetTouches::default()
    };
    for (name, sites) in recorded.tests {
        if recording.ran.iter().any(|test| test == &name) {
            drop(gathered.tests.insert(name, sites));
        } else {
            gathered.loose.extend(sites);
        }
    }
    for (name, bodies) in recorded.bodies {
        if recording.ran.iter().any(|test| test == &name) {
            drop(gathered.bodies.insert(name, bodies));
        } else {
            gathered.loose_bodies.extend(bodies);
        }
    }
    trace.touch(crate::trace::TouchRecord {
        target: recording.target.to_owned(),
        tests: u32::try_from(gathered.tests.len()).unwrap_or(u32::MAX),
        sites: u32::try_from(
            gathered
                .tests
                .values()
                .flatten()
                .chain(gathered.loose.iter())
                .collect::<std::collections::BTreeSet<&u32>>()
                .len(),
        )
        .unwrap_or(u32::MAX),
        loose: u32::try_from(gathered.loose.len()).unwrap_or(u32::MAX),
    });
    drop(
        touched
            .targets
            .insert(recording.target.to_owned(), gathered),
    );
}

/// A target identity as one path segment, so two targets cannot name one file.
///
/// The readable part is for a person looking in the directory; the digest is
/// what makes it an identity, because two targets whose names differ only
/// where the readable part folds would otherwise share a log and each be read
/// as having reached what the other did.
fn slug(target: &str) -> String {
    let readable: String = target
        .chars()
        .map(|letter| {
            if letter.is_ascii_alphanumeric() {
                letter
            } else {
                '-'
            }
        })
        .collect();
    let digest = crate::id::digest(target.as_bytes());
    format!("{readable}-{}", digest.get(..16).unwrap_or(&digest))
}
