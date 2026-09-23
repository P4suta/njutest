// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine's routing layers, each asked about the mutant planted for it before the run believes anything a layer removes.

use rust_mutants::sentinel::Sighted;

use crate::assure::run::Request;
use crate::cli::Environment;
use crate::error::RunnerError;
use crate::scratch::Scratch;
use crate::trace::SentinelRecord;
use crate::ui::Notes;
use crate::watch::Watch;

/// Plants a mutant for every routing layer in the run's scratch directory, asks the engine how it routes each without running any, and stops the run at the first layer that did not route its own.
///
/// The crate is written to the run's own directory and never to the tree under test, and every answer is recorded in the trace.
///
/// # Errors
/// Returns [`RunnerError::Blind`] for the first planted mutant a layer did not route as it must, and the engine's failure to write, open, or prepare the planted crate.
pub fn stood(
    (request, toolchain): (&Request, &rust_mutants::cargo::Toolchain),
    environment: &Environment,
    scratch: &Scratch,
    (notes, watch): (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    notes.phase("sentinel")?;
    watch.trace.stage("sentinel");
    let sighted = rust_mutants::sentinel::sighted(
        rust_mutants::sentinel::Run {
            toolchain,
            open: rust_mutants::workspace::OpenOptions {
                trace: rust_mutants::trace::Recorder::disabled(),
                ..crate::assure::run::opening(request, environment)
            },
            options: &crate::assure::run::preparing(request)?,
        },
        &scratch.sentinel_dir(),
        watch.cancel,
    )?;
    for path in &sighted.kept {
        notes.note("kept", &path.display().to_string())?;
    }
    for sighting in &sighted.sightings {
        watch.trace.sentinel(SentinelRecord::of(sighting));
    }
    believed(&sighted)
}

/// Nothing when every planted mutant was routed the way its layer must, and otherwise the error naming the first that was not.
///
/// # Errors
/// Returns [`RunnerError::Blind`] naming the layer, the planted mutant, what the layer had to do with it, and what the engine did.
pub fn believed(sighted: &Sighted) -> Result<(), RunnerError> {
    let Some(blind) = sighted.blind() else {
        return Ok(());
    };
    Err(RunnerError::Blind {
        layer: blind.expectation.planted,
        mutant: blind.expectation.mutant.to_string(),
        expected: blind.expectation.expected.to_string(),
        routed: blind.routed(),
    })
}
