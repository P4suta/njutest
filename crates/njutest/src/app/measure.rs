// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest measure`: record what each test target enters and whether a second run of it enters the same, for `njutest select` to read.

use std::io::Write;

use rust_mutants::select::Standing;

use crate::assure::measure::{Measuring, measure};
use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Measure as Arguments};
use crate::config::Config;
use crate::error::RunnerError;
use crate::trace::Recorder;
use crate::watch::Watch;

/// Measures the tree and keeps what it established where `njutest select` reads it.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let config = match Config::load(root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let watch = Watch::new(&cancel, &trace);
    let measuring = Measuring {
        root,
        environment,
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        config: &config,
    };
    let directory = crate::reach::directory(&root.join(config.reports.directory.as_str()));
    let kept = measure(&measuring, watch).and_then(|measured| {
        let document = crate::reach::Document {
            schema: crate::reach::Schema::V1,
            measurement: measured.measurement,
        };
        crate::reach::keep(&directory, &document, &measured.sources)?;
        Ok::<_, RunnerError>(document)
    });
    match kept {
        Ok(document) => {
            super::say(stdout, &summary(&document.measurement))?;
            super::say(stdout, &format!("KEPT\t{}", directory.display()))?;
            Ok(EXIT_ASSURED)
        }
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            Ok(EXIT_ERROR)
        }
    }
}

/// One line saying how much the measurement holds, and how much of it a selection may skip by.
fn summary(measurement: &rust_mutants::select::Measurement) -> String {
    let held = measurement
        .targets
        .values()
        .filter(|target| target.standing == Standing::Held)
        .count();
    format!(
        "MEASURED\t{} targets\t{held} held\t{} items\t{} files",
        measurement.targets.len(),
        measurement.items.len(),
        measurement.survey.files.len()
    )
}
