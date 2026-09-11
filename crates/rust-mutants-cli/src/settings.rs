// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a command actually runs with: the configuration file, and the flags that override it.

use std::path::{Path, PathBuf};

use rust_mutants::EngineError;
use rust_mutants::glob::Pattern;
use rust_mutants::session::PrepareOptions;
use rust_mutants::workspace::OpenOptions;

use crate::config::{Config, ConfigError, FILE_NAME};
use crate::error::CliError;
use crate::{Environment, cli};

/// The configuration a command runs with, and where it came from.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The workspace root.
    pub root: PathBuf,
    /// The configuration file that was read, when one was.
    pub source: Option<PathBuf>,
    /// The configuration, with every flag already folded in.
    pub config: Config,
}

impl Settings {
    /// Reads the configuration a scope names and folds the flags into it. A flag given on the command line wins over the file; a list given on the command line replaces the file's list rather than adding to it.
    ///
    /// A `--root` that is not an absolute path is resolved against the working
    /// directory the command was given rather than against the process's own.
    /// The two are the same for the binary, which composes one from the other,
    /// and they are not the same for anything else that calls this: a caller
    /// that says where it is and then gets an answer about somewhere else has
    /// been told about a tree it did not name.
    ///
    /// # Errors
    /// Returns what is wrong with the configuration, or with a duration a flag spells.
    pub fn resolve(scope: &cli::Scope, environment: &Environment) -> Result<Self, CliError> {
        let root = environment.rooted(scope.root.as_deref());
        let (source, mut config) = read(scope, &root)?;
        if let Some(tier) = scope.tier {
            config.mutation.tier = tier.tier();
        }
        replace(&mut config.mutation.operators, &scope.operators);
        replace(&mut config.project.include, &scope.include);
        replace(&mut config.project.exclude, &scope.exclude);
        replace(&mut config.project.packages, &scope.packages);
        config
            .execution
            .skip_targets
            .extend(scope.skip_targets.iter().cloned());
        if let Some(text) = &scope.timeout {
            config.mutation.timeout =
                crate::config::parse_timeout(text).map_err(EngineError::from)?;
        }
        replace(&mut config.build.features, &scope.features);
        config.build.all_features |= scope.switches.all_features;
        config.build.no_default_features |= scope.switches.no_default_features;
        if let Some(target) = &scope.build_target {
            config.build.target.clone_from(target);
        }
        if let Some(profile) = &scope.profile {
            config.build.profile.clone_from(profile);
        }
        if let Some(jobs) = scope.build_jobs {
            config.build.jobs = jobs;
        }
        if let Some(jobs) = scope.jobs {
            config.execution.jobs = jobs;
        }
        config.execution.offline |= scope.switches.offline;
        config.execution.locked |= scope.switches.locked;
        config.mutation.verify &= !scope.switches.no_verify;
        config.mutation.coverage |= scope.switches.coverage;
        config.mutation.coverage &= !scope.switches.no_coverage;
        config.mutation.touch &= !scope.switches.no_touch;
        config.mutation.equivalence |= scope.switches.equivalence;
        config.execution.doctests &= !scope.switches.no_doctests;
        Ok(Self {
            root,
            source,
            config,
        })
    }

    /// How the workspace is opened.
    ///
    /// # Errors
    /// Returns a pattern that is not a pattern.
    pub fn open_options(
        &self,
        scope: &cli::Scope,
        environment: &Environment,
        trace: rust_mutants::trace::Recorder,
    ) -> Result<OpenOptions, EngineError> {
        Ok(OpenOptions {
            cargo: None,
            search_path: environment
                .vars
                .iter()
                .find(|(name, _)| name == "PATH")
                .map(|(_, value)| value.clone()),
            env: environment.vars.clone(),
            temp_directory: environment.temp_directory.clone(),
            report_directory: Some(self.config.reports.directory.to_string_lossy().into_owned()),
            exclude: compile(&self.config.project.exclude)?,
            allow_outside: self
                .config
                .project
                .allow_outside
                .iter()
                .map(|path| self.root.join(path))
                .chain(scope.allow_outside.iter().cloned())
                .collect(),
            keep_temp: scope.switches.keep_temp,
            offline: self.config.execution.offline,
            locked: self.config.execution.locked,
            trace,
        })
    }

    /// How the mutants are proposed and validated.
    ///
    /// # Errors
    /// Returns a pattern that is not a pattern.
    pub fn prepare_options(&self) -> Result<PrepareOptions, EngineError> {
        Ok(PrepareOptions {
            tier: self.config.mutation.tier,
            operators: self.config.mutation.operators.clone(),
            include: compile(&self.config.project.include)?,
            exclude: compile(&self.config.project.exclude)?,
            harness_args: self.config.execution.test_binary_args.clone(),
            packages: self.config.project.packages.clone(),
            verify: self.config.mutation.verify,
            coverage: self.config.mutation.coverage,
            touch: self.config.mutation.touch,
            branch_proofs: self.config.mutation.coverage || self.config.mutation.touch,
            build_timeout: self.config.mutation.build_timeout,
            mutant_timeout: self.config.mutation.timeout,
            doctests: self.config.execution.doctests,
            build: self.config.build.config(),
            skip_targets: self.config.execution.skip_targets.clone(),
            skips: self
                .config
                .mutation
                .skip
                .iter()
                .map(crate::config::Skip::rule)
                .collect::<Result<Vec<_>, _>>()?,
            ..PrepareOptions::default()
        })
    }

    /// Where run reports go.
    #[must_use]
    pub fn report_directory(&self) -> PathBuf {
        self.root.join(&self.config.reports.directory)
    }
}

fn read(scope: &cli::Scope, root: &Path) -> Result<(Option<PathBuf>, Config), ConfigError> {
    if scope.no_config {
        return Ok((None, Config::default()));
    }
    if let Some(path) = &scope.config {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ConfigError::unreadable(path, error.to_string()))?;
        return Ok((Some(path.clone()), Config::parse(&text, path)?));
    }
    let path = root.join(FILE_NAME);
    let config = Config::load(root)?;
    Ok((path.is_file().then_some(path), config))
}

fn replace(field: &mut Vec<String>, given: &[String]) {
    if !given.is_empty() {
        field.clear();
        field.extend_from_slice(given);
    }
}

fn compile(patterns: &[String]) -> Result<Vec<Pattern>, EngineError> {
    patterns
        .iter()
        .map(|pattern| Pattern::compile(pattern).map_err(EngineError::from))
        .collect()
}
