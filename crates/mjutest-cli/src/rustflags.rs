// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a build is compiled with, as the engine reads it.

use std::ffi::OsString;
use std::path::Path;

pub use rust_mutants::cargo::config::{Configured, SEPARATOR, encoded, home};

/// The flag the coverage build adds.
pub const COVERAGE_FLAG: &str = "-Cinstrument-coverage";

/// The limitation a run states when the project configures flags for a target and the coverage build could not merge them.
pub const TARGET_RUSTFLAGS_LIMITATION: &str = "target-rustflags-not-merged";

/// The limitation a run states when a cargo configuration file could not be read at all.
pub const UNREADABLE_CONFIG_LIMITATION: &str =
    rust_mutants::limitation::CARGO_CONFIGURATION_UNREADABLE;

/// Reads the cargo configuration a build in `root` would compile under, including the one `env` says the home directory holds.
#[must_use]
pub fn configured(root: &Path, env: &[(OsString, OsString)]) -> Configured {
    rust_mutants::cargo::config::configured(root, home(env).as_deref())
}
