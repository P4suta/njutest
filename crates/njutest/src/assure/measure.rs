// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Measuring what each target of a tree enters, and whether a second whole run of it enters the same, for a selection to read.

use std::collections::BTreeMap;
use std::path::Path;

use rust_mutants::select::{Measurement, Parts};
use rust_mutants::session::{Observing, PrepareOptions, Request, Timeout};
use rust_mutants::workspace::{OpenOptions, Workspace};

use crate::build::Cargo;
use crate::cli::Environment;
use crate::config::Config;
use crate::error::{ErrorCode, RunnerError};
use crate::watch::Watch;

/// What one measurement needs.
#[derive(Debug, Clone, Copy)]
pub struct Measuring<'a> {
    /// The tree, which is only ever read.
    pub root: &'a Path,
    /// The environment every command and test process runs with.
    pub environment: &'a Environment,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// The configuration a run of this tree reads.
    pub config: &'a Config,
}

/// A measurement, and the measured bytes of every source file it names, by SHA-256.
#[derive(Debug, Clone)]
pub struct Measured {
    /// What the tree establishes.
    pub measurement: Measurement,
    /// The bytes a selection compares a changed file with, by their digest.
    pub sources: BTreeMap<String, Vec<u8>>,
}

/// Why a tree could not be measured, beyond what the engine refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MeasureError {
    /// A file is not what the copy holds, or changed after the copy was made.
    #[error(
        "{}: {path} changed while the tree was being measured",
        crate::error::TREE_WRITTEN_DURING_MEASUREMENT.code
    )]
    Written {
        /// The file, relative to the tree.
        path: String,
    },
    /// A variable the run selects is not UTF-8.
    #[error("{}: {source}", crate::error::CONFIG_INVALID.code)]
    Environment {
        /// What was refused.
        #[from]
        source: crate::assure::identity::EnvironmentError,
    },
}

impl MeasureError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Written { .. } => crate::error::TREE_WRITTEN_DURING_MEASUREMENT,
            Self::Environment { .. } => crate::error::CONFIG_INVALID,
        }
    }
}

/// The options a workspace is opened with, which a selection surveys the tree by as well.
#[must_use]
pub fn opening(measuring: &Measuring<'_>) -> OpenOptions {
    OpenOptions {
        allow_outside: Vec::new(),
        cargo: None,
        search_path: measuring
            .environment
            .var("PATH")
            .map(std::ffi::OsStr::to_owned),
        env: measuring.environment.vars.clone(),
        temp_directory: measuring.environment.temp_directory.clone(),
        report_directory: Some(measuring.config.reports.directory.as_str().to_owned()),
        exclude: Vec::new(),
        keep_temp: false,
        offline: measuring.cargo.offline,
        locked: measuring.cargo.locked,
        trace: rust_mutants::trace::Recorder::disabled(),
    }
}

/// What a run of this tree is compiled and started with, by name.
#[must_use]
pub fn settings(config: &Config) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "build".to_owned(),
            config
                .execution
                .build()
                .selection()
                .digest()
                .as_str()
                .to_owned(),
        ),
        (
            "harness".to_owned(),
            config.execution.test_binary_args.join("\u{1f}"),
        ),
        (
            "skipped".to_owned(),
            config.execution.skip_targets.join("\u{1f}"),
        ),
    ])
}

/// The variables a run of this tree selects for its test processes.
///
/// # Errors
/// [`MeasureError::Environment`] when one is not UTF-8.
pub fn environment(
    environment: &Environment,
    config: &Config,
) -> Result<BTreeMap<String, String>, MeasureError> {
    Ok(
        crate::assure::identity::environment_of(&environment.vars, config)?
            .into_iter()
            .collect(),
    )
}

/// Measures the tree: one instrumented baseline of every target, then one whole run of each again, alone, to compare with it.
///
/// # Errors
/// What the engine refuses, a tree written while it was read, and a selected variable that is not UTF-8.
pub fn measure(measuring: &Measuring<'_>, watch: Watch<'_>) -> Result<Measured, RunnerError> {
    let config = measuring.config;
    let workspace = Workspace::open(measuring.root, opening(measuring), watch.cancel)?;
    let survey = workspace
        .survey()
        .map_err(rust_mutants::EngineError::from)?;
    as_copied(&workspace, &survey)?;
    let toolchain = workspace.toolchain().to_string();
    let session = workspace.prepare(
        &PrepareOptions {
            verify: true,
            touch: true,
            failing: rust_mutants::session::Failing::Exclude,
            build: config.execution.build(),
            harness_args: config.execution.test_binary_args.clone(),
            skip_targets: config.execution.skip_targets.clone(),
            build_timeout: config.execution.build_timeout,
            mutant_timeout: Timeout::Fixed(config.execution.timeout),
            mutant_steps: (config.execution.steps > 0).then_some(config.execution.steps),
            ..crate::assure::engine::switches()
        },
        watch.cancel,
    )?;
    let mut standing = BTreeMap::new();
    for target in session.touched().targets.keys() {
        if watch.cancel.is_cancelled() {
            return Err(RunnerError::Interrupted);
        }
        let request = Request::new(String::new()).with_target(target.as_str());
        for observed in session
            .control(&request, watch.cancel, Observing::Reach)?
            .observed
        {
            standing.insert(observed.target, observed.steadiness);
        }
    }
    let mut texts: Vec<(String, String)> = Vec::new();
    let mut sources = BTreeMap::new();
    for file in session.files() {
        let Some(surveyed) = survey.files.get(&file.path) else {
            return Err(MeasureError::Written {
                path: file.path.clone(),
            }
            .into());
        };
        let bytes = std::fs::read(measuring.root.join(&file.path)).map_err(|_gone| {
            MeasureError::Written {
                path: file.path.clone(),
            }
        })?;
        if rust_mutants::id::digest(&bytes) != surveyed.sha256 {
            return Err(MeasureError::Written {
                path: file.path.clone(),
            }
            .into());
        }
        let text = String::from_utf8(bytes.clone()).map_err(|_not_text| MeasureError::Written {
            path: file.path.clone(),
        })?;
        texts.push((file.package.clone(), text));
        sources.insert(surveyed.sha256.clone(), bytes);
    }
    let measurement = Measurement::of(
        Parts {
            toolchain: &toolchain,
            survey: &survey,
            inputs: session.inputs(),
            environment: &environment(measuring.environment, config)?,
            settings: &settings(config),
            touched: session.touched(),
            standing: &standing,
        },
        texts
            .iter()
            .map(|(package, text)| (package.as_str(), text.as_str())),
    );
    session.close()?;
    Ok(Measured {
        measurement,
        sources,
    })
}

/// Holds the survey of the source to what the copy holds, so what a measurement says a file was is what the build read.
fn as_copied(
    workspace: &Workspace,
    survey: &rust_mutants::snapshot::Survey,
) -> Result<(), MeasureError> {
    let copied: BTreeMap<&str, &str> = workspace
        .copied()
        .iter()
        .map(|entry| (entry.rel_path.as_str(), entry.sha256.as_str()))
        .collect();
    let surveyed: BTreeMap<&str, &str> = survey
        .files
        .iter()
        .filter(|(path, _)| !Path::new(path).is_absolute())
        .map(|(path, file)| (path.as_str(), file.sha256.as_str()))
        .collect();
    match copied
        .keys()
        .chain(surveyed.keys())
        .find(|path| copied.get(*path) != surveyed.get(*path))
    {
        Some(path) => Err(MeasureError::Written {
            path: (*path).to_owned(),
        }),
        None => Ok(()),
    }
}
