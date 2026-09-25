// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a command actually runs with: the configuration file, and the flags that override it.

use std::path::{Path, PathBuf};

use rust_mutants::EngineError;
use rust_mutants::glob::Pattern;
use rust_mutants::session::PrepareOptions;
use rust_mutants::workspace::OpenOptions;

use crate::config::{Config, ConfigError};
use crate::error::CliError;
use crate::{Environment, cli};

/// The configuration a command runs with, and where it came from.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The workspace root.
    pub root: PathBuf,
    /// The configuration, with every flag already folded in.
    pub config: Config,
}

impl Settings {
    /// Reads the configuration a scope names and folds the flags into it.
    /// A flag given on the command line wins over the file; a list given on the command line replaces the file's list rather than adding to it.
    ///
    /// # Errors
    /// Returns what is wrong with the configuration, or with a duration a flag spells.
    pub fn resolve(scope: &cli::Scope, environment: &Environment) -> Result<Self, CliError> {
        let root = environment.rooted(scope.root.as_deref());
        let mut config = read(scope, &root)?;
        if let Some(tier) = scope.tier {
            config.mutation.tier = tier.tier();
        }
        replace(&mut config.mutation.operators, &scope.operators);
        replace(&mut config.project.include, &scope.include);
        replace(&mut config.project.exclude, &scope.exclude);
        replace(&mut config.snapshot.omit, &scope.omit);
        replace(&mut config.project.packages, &scope.packages);
        replace(&mut config.execution.skip_targets, &scope.skip_targets);
        if let Some(text) = &scope.timeout {
            config.mutation.timeout =
                crate::config::parse_timeout(text).map_err(EngineError::from)?;
        }
        replace(&mut config.build.features, &scope.features);
        let cli::Switches {
            offline,
            locked,
            keep_temp: _read_when_the_workspace_is_opened,
            no_verify,
            coverage,
            no_coverage,
            no_touch,
            equivalence,
            no_doctests,
            all_features,
            no_default_features,
        } = scope.switches;
        config.build.all_features |= all_features;
        config.build.no_default_features |= no_default_features;
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
            config.execution.jobs = Some(jobs);
        }
        config.execution.offline |= offline;
        config.execution.locked |= locked;
        config.mutation.verify &= !no_verify;
        config.mutation.coverage |= coverage;
        config.mutation.coverage &= !no_coverage;
        config.mutation.touch &= !no_touch;
        config.mutation.equivalence |= equivalence;
        config.execution.doctests &= !no_doctests;
        Ok(Self { root, config })
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
        let report_directory = rust_mutants::id::slashed(&self.config.reports.directory)
            .map_err(rust_mutants::workspace::SessionError::from)?;
        Ok(OpenOptions {
            cargo: None,
            search_path: rust_mutants::vars::search_path(&environment.vars),
            env: environment.vars.clone(),
            temp_directory: environment.temp_directory.clone(),
            report_directory: Some(report_directory),
            exclude: compile(&self.config.snapshot.omit)?,
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
            narrowing: Vec::new(),
            harness_args: self.config.execution.test_binary_args.clone(),
            scratch_working_directory: self.config.execution.scratch_working_directory,
            packages: self.config.project.packages.clone(),
            verify: self.config.mutation.verify,
            coverage: self.config.mutation.coverage,
            touch: self.config.mutation.touch,
            branch_proofs: self.config.mutation.coverage || self.config.mutation.touch,
            build_timeout: self.config.mutation.build_timeout,
            mutant_timeout: self.config.mutation.timeout,
            mutant_steps: (self.config.mutation.steps > 0).then_some(self.config.mutation.steps),
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
            measurements: None,
            failing: rust_mutants::session::Failing::Refuse,
            max_rounds: rust_mutants::validate::DEFAULT_MAX_ROUNDS,
            validation_filter: None,
        })
    }

    /// Where run reports go.
    #[must_use]
    pub fn report_directory(&self) -> PathBuf {
        crate::app::stored::Store::of(&self.root, &self.config.reports.directory).root()
    }
}

fn read(scope: &cli::Scope, root: &Path) -> Result<Config, ConfigError> {
    if scope.no_config {
        return Ok(Config::default());
    }
    if let Some(path) = &scope.config {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ConfigError::unreadable(path, error.to_string()))?;
        return Config::parse(&text, path);
    }
    Config::load(root)
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
