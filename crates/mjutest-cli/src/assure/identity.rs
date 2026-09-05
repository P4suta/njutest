// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assembling what a run is, as one number, from the tree and the machine it runs on.

use std::ffi::OsString;
use std::path::Path;

use crate::config::Config;
use crate::evidence::digest::{Inputs, Mode, identity};
use crate::evidence::tree::{ScanError, dependencies_of, scan};

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
}

impl Evidence {
    /// Whether the numbers were established rather than left unsaid.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        !self.identity.is_empty() && !self.tree.is_empty()
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
}

/// Everything the identity is computed from, read from the tree and the process.
///
/// # Errors
/// Returns what could not be read about the tree.
pub fn inputs(asked: &Asked<'_>, mode: Mode) -> Result<Inputs, ScanError> {
    let Asked {
        root,
        config,
        machine,
        vars,
        elsewhere,
    } = *asked;
    let exclude = compiled(config);
    let scanned = scan(root, &exclude, elsewhere)?;
    Ok(Inputs {
        tree: scanned.tree,
        corpus: scanned.corpus,
        dependencies: dependencies_of(root)?,
        toolchain: machine.toolchain.to_owned(),
        platform: machine.platform.to_owned(),
        environment: selected(vars, &config.execution.environment),
        contract: config.contract,
        configuration: config.digest(),
        mode,
    })
}

/// What one run is: [`inputs`] folded, and the tree digest beside it.
///
/// # Errors
/// Returns what could not be read about the tree.
pub fn of(asked: &Asked<'_>, mode: Mode) -> Result<Evidence, ScanError> {
    let read = inputs(asked, mode)?;
    Ok(Evidence {
        identity: identity(&read),
        tree: read.tree.clone(),
    })
}

/// A pattern the configuration writes that does not compile cannot reach here: the configuration refuses it when it is read. One that somehow does is left out of the exclusions, which widens what the identity covers rather than narrowing it.
fn compiled(config: &Config) -> Vec<rust_mutants::glob::Pattern> {
    config
        .project
        .exclude
        .iter()
        .filter_map(|pattern| rust_mutants::glob::Pattern::compile(pattern).ok())
        .collect()
}

/// The environment the run is a function of: the variables that change what the compiler produces, plus whatever the configuration named.
fn selected(vars: &[(OsString, OsString)], named: &[String]) -> Vec<(String, String)> {
    vars.iter()
        .filter_map(|(name, value)| {
            let name = name.to_str()?;
            let wanted = SELECTED_NAMES.contains(&name)
                || SELECTED_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                || named.iter().any(|one| one == name);
            wanted.then(|| (name.to_owned(), value.to_string_lossy().into_owned()))
        })
        .collect()
}
