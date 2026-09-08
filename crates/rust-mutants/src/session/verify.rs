// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one run of every target with nothing active: the baseline check, and the measurement that rides on it.
//!
//! Every guard of the instrumented tree runs on it, and libtest names each
//! test's thread after the test, so asking the guards what they reached costs
//! the run nothing it was not already spending. What they say is read here,
//! and a target this run cannot ask is named rather than read as having
//! reached nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use super::prepare::Building;
use crate::EngineError;
use crate::catalog::Catalog;
use crate::execute::{self, Context, ExecRequest, MutantResult, TargetKind, TestTarget};
use crate::workspace::{SessionError, Workspace};

/// Runs every target once with nothing active. A tree whose instrumented baseline fails is one whose every later result would be about the instrumentation rather than about a mutant.
pub(super) fn verify(
    workspace: &Workspace,
    targets: &mut [TestTarget],
    scratch: &std::path::Path,
    building: &Building<'_>,
) -> Result<Verified, EngineError> {
    let Building { catalog, asked, .. } = *building;
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
        let mut result = ran(target, scratch, recording.as_deref(), building);
        let recording = if result.exit_code == crate::instrument::TOUCH_UNAVAILABLE_EXIT {
            workspace.trace.note(
                crate::touch::UNRECORDED,
                &format!(
                    "{}: the process could not write what its guards reached, so it is run \
                     again with nothing to record and every test of it stays in every route",
                    target.id
                ),
            );
            result = ran(target, scratch, None, building);
            None
        } else {
            recording
        };
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

/// One target run with nothing active, recording into `log` when it was asked to.
///
/// The log is removed first: the directory outlives a run, and a record two
/// runs both appended to would say the older one's touches were this one's.
fn ran(
    target: &TestTarget,
    scratch: &std::path::Path,
    log: Option<&std::path::Path>,
    building: &Building<'_>,
) -> MutantResult {
    let Building {
        cancel,
        workspace,
        catalog,
        ..
    } = *building;
    if let Some(path) = log {
        drop(std::fs::remove_file(path));
    }
    let context = Context {
        base_env: &workspace.base_env,
        cargo: Some(workspace.toolchain.cargo()),
        sysroot: workspace.toolchain.sysroot(),
        active: None,
        probe: None,
        touch: log.map(|log| execute::Touching {
            log,
            catalog: catalog.digest(),
        }),
        profile: None,
    };
    let request = ExecRequest::new(target).with_scratch(scratch);
    execute::exec(&request, &context, cancel, &workspace.trace)
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
/// appends the first time any guard of the process runs, so a file that is not
/// there is a process whose guards never ran rather than a process that was
/// never asked. A file that is there and cannot be read is neither, and is
/// read as neither. A record naming a thread the run does not know as one of
/// its tests is a touch nothing can be attributed to, and reaches every test
/// of the target.
fn gather(
    touched: &mut crate::touch::Touched,
    recording: &Recording<'_>,
    trace: &crate::trace::Recorder,
) {
    let Some(log) = recording.log else {
        touched.limited(crate::touch::UNRECORDED, recording.target);
        return;
    };
    let unreadable = |touched: &mut crate::touch::Touched, why: &dyn std::fmt::Display| {
        trace.note(
            crate::touch::UNREADABLE,
            &format!("{}: {why}", recording.target),
        );
        touched.limited(crate::touch::UNREADABLE, recording.target);
    };
    let text = match crate::limitation::appended(std::fs::read_to_string(log)) {
        Ok(text) => text,
        Err(error) => {
            unreadable(touched, &error);
            return;
        }
    };
    let count = u32::try_from(recording.catalog.mutants().len()).unwrap_or(u32::MAX);
    let recorded = match crate::touch::read(&text, recording.catalog.digest(), count) {
        Ok(recorded) => recorded,
        Err(error) => {
            unreadable(touched, &error);
            return;
        }
    };
    let gathered = crate::touch::TargetTouches {
        reached: attributed(recorded.reached, recording.ran),
        bodies: attributed(recorded.bodies, recording.ran),
        infected: attributed(recorded.infected, recording.ran),
        ran: recording.ran.to_vec(),
    };
    trace.touch(crate::trace::TouchRecord {
        target: recording.target.to_owned(),
        tests: counted(gathered.reached.tests.len()),
        sites: counted(
            gathered
                .reached
                .tests
                .values()
                .flatten()
                .chain(gathered.reached.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
        loose: counted(gathered.reached.loose.len()),
        infected: counted(
            gathered
                .infected
                .tests
                .values()
                .flatten()
                .chain(gathered.infected.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
    });
    drop(
        touched
            .targets
            .insert(recording.target.to_owned(), gathered),
    );
}

/// A count as the wire carries it.
fn counted(many: usize) -> u32 {
    u32::try_from(many).unwrap_or(u32::MAX)
}

/// What each test of the target reached, with everything else folded into `loose`.
///
/// A record names the thread that made it, and libtest names a test's thread
/// after the test — but a thread the run does not know as one of its tests is
/// a thread nothing can be attributed to, whatever it called itself. What it
/// reached goes where the unattributable goes, and reaches every test of the
/// target.
fn attributed(recorded: crate::touch::Seen, ran: &[String]) -> crate::touch::Seen {
    let mut held = crate::touch::Seen {
        loose: recorded.loose,
        ..crate::touch::Seen::default()
    };
    for (thread, reported) in recorded.tests {
        if ran.iter().any(|test| test == &thread) {
            drop(held.tests.insert(thread, reported));
        } else {
            held.loose.extend(reported);
        }
    }
    held
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
