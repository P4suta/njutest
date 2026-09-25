// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assembling what a run is, as one number, from the tree and the machine it runs on.

use std::ffi::OsString;
use std::path::Path;

use crate::config::{Config, ConfigError};
use crate::evidence::digest::{Inputs, Mode, identity};
use crate::evidence::key::{self, Common};
use crate::evidence::tree::{Scan, ScanError, dependencies_of, scan};

/// Why a selected environment entry cannot be represented in the canonical run identity.
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    /// The variable name is not valid UTF-8.
    #[error("environment variable name {name:?} is not valid UTF-8: {source}")]
    Name {
        /// The exact platform spelling.
        name: OsString,
        /// Why the encoded bytes are not UTF-8.
        #[source]
        source: std::str::Utf8Error,
    },
    /// A selected variable's value is not valid UTF-8.
    #[error("selected environment variable {name} is not valid UTF-8: {source}")]
    Value {
        /// The selected variable.
        name: String,
        /// Why the encoded bytes are not UTF-8.
        #[source]
        source: std::str::Utf8Error,
    },
}

/// Why the inputs of a run could not be given one exact identity.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The verified tree or its dependency lock could not be read exactly.
    #[error(transparent)]
    Scan(#[from] ScanError),
    /// The validated configuration could not be rendered canonically.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A selected environment entry cannot be represented exactly.
    #[error(transparent)]
    Environment(#[from] EnvironmentError),
}

impl From<IdentityError> for crate::error::RunnerError {
    fn from(source: IdentityError) -> Self {
        match source {
            IdentityError::Scan(source) => Self::Evidence(source),
            IdentityError::Config(source) => Self::Config(source),
            IdentityError::Environment(source) => Self::IdentityEnvironment { source },
        }
    }
}

/// Environment variables that change what the compiler produces, by name.
pub const SELECTED_NAMES: [&str; 9] = [
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CC",
    "CXX",
    "SOURCE_DATE_EPOCH",
];

/// Environment variables that change what the compiler produces, by prefix.
pub const SELECTED_PREFIXES: [&str; 2] = ["CARGO_PROFILE_", "CARGO_TARGET_"];

/// What one run is, as numbers: the identity a later run compares against, and the tree digest a report records.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Evidence {
    /// The identity of everything that could change what the tests say.
    pub identity: String,
    /// The digest of the tree under verification alone.
    pub tree: String,
    /// What a behaviour key is computed from, when the tree could be read.
    pub keying: Option<Keying>,
}

/// What every behaviour key of one run is computed from: the tree it read, what the lock file resolved, and what every key of the run shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keying {
    /// The tree, file by file.
    pub scan: Scan,
    /// The digest of the resolved dependencies.
    pub dependencies: String,
    /// What every key shares.
    pub common: Common,
}

impl Evidence {
    /// Whether the numbers were established rather than left unsaid.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        !self.identity.is_empty() && !self.tree.is_empty()
    }

    /// Rebinds only build-local cache and continuation inputs while retaining the run-wide identity shared by the configured-build report.
    ///
    /// An unknown evidence value remains unknown.
    /// A known one owns its scan and dependency digest already, so rebinding does not reread a tree that tests may have begun to execute against.
    #[must_use]
    pub fn for_build(&self, build: &rust_mutants::cargo::BuildConfig) -> Self {
        let mut rebound = self.clone();
        if let Some(keying) = &mut rebound.keying {
            keying.common.build = build.selection();
        }
        rebound
    }

    /// The identity of resumable state for this selected build.
    ///
    /// The report and completed-run cache use [`Self::identity`], which binds the whole configured request.
    /// Checkpoints are made before the builds are reconciled and therefore add the active build as a separate,
    /// typed domain.
    /// Falling back to the run identity is safe only for an unknown evidence value, which callers already refuse to persist.
    #[must_use]
    pub fn continuation_identity(&self) -> String {
        self.keying.as_ref().map_or_else(
            || self.identity.clone(),
            |keying| key::continuation_identity(&self.identity, &keying.common.build),
        )
    }
}

/// The tree an identity is read from, and the machine it is read on.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The effective configuration.
    pub config: &'a Config,
    /// The machine the run happens on.
    pub machine: &'a Machine<'a>,
    /// The process environment.
    pub vars: &'a [(OsString, OsString)],
    /// Directories a run writes rather than reads that the standing exclusions do not cover: cargo's build directory, the user's cache directory.
    pub elsewhere: &'a [&'a Path],
}

/// The machine a run happens on, as the identity reads it.
#[derive(Debug, Clone, Copy)]
pub struct Machine<'a> {
    /// The toolchain, as it names itself.
    pub toolchain: &'a str,
    /// The target triple.
    pub platform: &'a str,
    /// The digest of the running njutest, because two builds of it may mean two different things by the same answer.
    pub engine: &'a str,
}

/// Everything the identity is computed from, read from the tree and the process.
///
/// # Errors
/// Returns what could not be read about the tree.
pub fn inputs(
    asked: &Asked<'_>,
    mode: Mode,
    test_args: &[String],
    shard: Option<String>,
) -> Result<Inputs, IdentityError> {
    let Asked {
        root,
        config,
        machine,
        vars,
        elsewhere,
    } = *asked;
    let excluded = crate::evidence::tree::Excluded::beside(config.reports.directory.as_path())?;
    let scanned = scan(
        root,
        &crate::evidence::tree::Bounds {
            exclude: &[],
            elsewhere,
            excluded: &excluded,
        },
    )?;
    Ok(Inputs {
        tree: scanned.tree,
        corpus: scanned.corpus,
        dependencies: dependencies_of(root)?,
        toolchain: machine.toolchain.to_owned(),
        platform: machine.platform.to_owned(),
        engine: machine.engine.to_owned(),
        environment: selected(vars, &config.execution.environment)?,
        contract: config.contract,
        configuration: config.digest()?,
        test_args: test_args.to_vec(),
        mode,
        shard,
    })
}

/// What one run is: [`inputs`] folded, and the tree digest beside it.
///
/// # Errors
/// Returns what could not be read about the tree.
pub fn of(
    asked: &Asked<'_>,
    mode: Mode,
    common: Common,
    shard: Option<String>,
) -> Result<Evidence, IdentityError> {
    let excluded =
        crate::evidence::tree::Excluded::beside(asked.config.reports.directory.as_path())?;
    let scanned = scan(
        asked.root,
        &crate::evidence::tree::Bounds {
            exclude: &[],
            elsewhere: asked.elsewhere,
            excluded: &excluded,
        },
    )?;
    let dependencies = dependencies_of(asked.root)?;
    let read = Inputs {
        tree: scanned.tree.clone(),
        corpus: scanned.corpus.clone(),
        dependencies: dependencies.clone(),
        toolchain: asked.machine.toolchain.to_owned(),
        platform: asked.machine.platform.to_owned(),
        engine: asked.machine.engine.to_owned(),
        environment: selected(asked.vars, &asked.config.execution.environment)?,
        contract: asked.config.contract,
        configuration: asked.config.digest()?,
        test_args: common.test_args.clone(),
        mode,
        shard,
    };
    Ok(Evidence {
        identity: identity(&read),
        tree: read.tree.clone(),
        keying: Some(Keying {
            scan: scanned,
            dependencies,
            common,
        }),
    })
}

/// The environment the run is a function of: the variables that change what the compiler produces, plus whatever the configuration named.
fn selected(
    vars: &[(OsString, OsString)],
    named: &[String],
) -> Result<Vec<(String, String)>, EnvironmentError> {
    let mut selected = Vec::new();
    for (name, value) in vars {
        let wanted_by_bytes = SELECTED_NAMES
            .iter()
            .any(|one| one.as_bytes() == name.as_encoded_bytes())
            || SELECTED_PREFIXES
                .iter()
                .any(|prefix| name.as_encoded_bytes().starts_with(prefix.as_bytes()))
            || named
                .iter()
                .any(|one| one.as_bytes() == name.as_encoded_bytes());
        if !wanted_by_bytes {
            continue;
        }
        let name_text = std::str::from_utf8(name.as_encoded_bytes()).map_err(|source| {
            EnvironmentError::Name {
                name: name.clone(),
                source,
            }
        })?;
        let value = std::str::from_utf8(value.as_encoded_bytes()).map_err(|source| {
            EnvironmentError::Value {
                name: name_text.to_owned(),
                source,
            }
        })?;
        selected.push((name_text.to_owned(), value.to_owned()));
    }
    Ok(selected)
}
