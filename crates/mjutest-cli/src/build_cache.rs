// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two layers of build output, and the one rule that says which a command writes into.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{self, ErrorCode};

/// The layout version, in the directory name: a later layout is a new directory, not a migration.
pub const LAYOUT_DIR: &str = "build-v1";

/// The marker that proves a directory is one this program made.
pub const MARKER_SCHEMA: &str = "mjutest-build-cache-v1";

/// The file that marker is written to.
pub const MARKER_NAME: &str = "mjutest-build-cache-v1.json";

/// One flavour of build output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// What `cargo test --no-run` and the doctest build produce.
    Native,
    /// The `-C instrument-coverage` build.
    Coverage,
    /// What the engine builds from its instrumented snapshot.
    Mutants,
}

impl Layer {
    /// The directory name of this layer.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Coverage => "coverage",
            Self::Mutants => "mutants",
        }
    }
}

/// Every command a run starts that a build directory could belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// `cargo metadata`.
    Metadata,
    /// `cargo test --no-run`.
    TestBuild,
    /// The doctest build.
    DoctestBuild,
    /// The `-C instrument-coverage` build.
    CoverageBuild,
    /// What the engine compiles from its snapshot.
    EngineBuild,
    /// One baseline target's test binary.
    TargetProcess,
    /// An original control: the same binary on unmutated code.
    ControlProcess,
    /// One mutant execution.
    MutantProcess,
    /// A resource or generation provider.
    ProviderProcess,
    /// `llvm-profdata` or `llvm-cov`.
    LlvmTool,
}

/// Every command, so a test can enumerate them.
pub const COMMANDS: [Command; 10] = [
    Command::Metadata,
    Command::TestBuild,
    Command::DoctestBuild,
    Command::CoverageBuild,
    Command::EngineBuild,
    Command::TargetProcess,
    Command::ControlProcess,
    Command::MutantProcess,
    Command::ProviderProcess,
    Command::LlvmTool,
];

/// Where one command's build output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Destination {
    /// The layer that survives the run.
    Base(Layer),
    /// The layer that dies with the run.
    Scratch,
    /// Nowhere: the command builds nothing.
    Nowhere,
}

/// The rule of [ADR 0005], in one function.
#[must_use]
pub const fn layer_for(command: Command) -> Destination {
    match command {
        Command::Metadata | Command::TestBuild | Command::DoctestBuild => {
            Destination::Base(Layer::Native)
        }
        Command::CoverageBuild => Destination::Base(Layer::Coverage),
        Command::EngineBuild => Destination::Base(Layer::Mutants),
        Command::TargetProcess
        | Command::ControlProcess
        | Command::MutantProcess
        | Command::ProviderProcess => Destination::Scratch,
        Command::LlvmTool => Destination::Nowhere,
    }
}

/// How a command is told where to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum How {
    /// `--target-dir DIR` on the command line, which outranks the variable.
    Flag,
    /// `CARGO_TARGET_DIR=DIR` in the environment, which any cargo the process spawns inherits.
    Environment,
}

/// Where one command builds, and how it is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The directory.
    pub dir: PathBuf,
    /// How to say so.
    pub how: How,
}

/// The marker of one prepared layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// [`MARKER_SCHEMA`].
    pub schema: String,
    /// Which flavour this directory holds.
    pub layer: Layer,
    /// The compiler its contents were built by.
    pub toolchain: String,
}

/// Why a layer could not be used. Never a reason to fail a run: the caller notes it and builds without one.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The directory holds files and carries none of this program's names, so it is somebody else's and is left exactly as it was found.
    #[error("{}: {path} holds files this program did not put there", error::BUILD_CACHE_UNUSABLE.code)]
    Foreign {
        /// The directory.
        path: PathBuf,
    },
    /// The directory or its marker could not be written.
    #[error("{}: preparing {path}: {source}", error::BUILD_CACHE_UNUSABLE.code)]
    Unusable {
        /// The directory.
        path: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
}

impl CacheError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Foreign { .. } | Self::Unusable { .. } => error::BUILD_CACHE_UNUSABLE,
        }
    }
}

/// The base layers of one machine, for one compiler.
#[derive(Debug, Clone)]
pub struct BuildCache {
    root: PathBuf,
    toolchain: String,
}

impl BuildCache {
    /// The layers under `build_dir`, for the compiler `toolchain` names — its commit hash, which is what actually decides whether artifacts are compatible.
    #[must_use]
    pub fn new(build_dir: &Path, toolchain: &str) -> Self {
        Self {
            root: build_dir.join(LAYOUT_DIR),
            toolchain: sanitize(toolchain),
        }
    }

    /// Where a layer is, whether or not it exists.
    #[must_use]
    pub fn dir(&self, layer: Layer) -> PathBuf {
        self.root.join(layer.dir_name()).join(&self.toolchain)
    }

    /// Makes the layer if it is not there and leaves the marker that says this program made it. Asking twice asks for the same directory.
    ///
    /// # Errors
    /// [`CacheError::Foreign`] for a directory holding files and none of
    /// this program's names, and [`CacheError::Unusable`] for the I/O
    /// failure.
    pub fn prepare(&self, layer: Layer) -> Result<PathBuf, CacheError> {
        let dir = self.dir(layer);
        if is_foreign(&dir) {
            return Err(CacheError::Foreign { path: dir });
        }
        fs::create_dir_all(&dir).map_err(|source| CacheError::Unusable {
            path: dir.clone(),
            source,
        })?;
        let marker = Marker {
            schema: MARKER_SCHEMA.to_owned(),
            layer,
            toolchain: self.toolchain.clone(),
        };
        let mut raw = serde_json::to_vec(&marker)
            .map_err(io::Error::other)
            .map_err(|source| CacheError::Unusable {
                path: dir.clone(),
                source,
            })?;
        raw.push(b'\n');
        fs::write(dir.join(MARKER_NAME), raw).map_err(|source| CacheError::Unusable {
            path: dir.clone(),
            source,
        })?;
        Ok(dir)
    }

    /// Where `command` builds and how it is told, given the run's scratch build directory. `None` for a command that builds nothing.
    #[must_use]
    pub fn placement(&self, command: Command, scratch_build_dir: &Path) -> Option<Placement> {
        match layer_for(command) {
            Destination::Base(layer) => Some(Placement {
                dir: self.dir(layer),
                how: How::Flag,
            }),
            Destination::Scratch => Some(Placement {
                dir: scratch_build_dir.to_path_buf(),
                how: How::Environment,
            }),
            Destination::Nowhere => None,
        }
    }
}

/// Whether `dir` holds files and none of this program's names. An empty directory is not foreign: a run may well have made it and died.
fn is_foreign(dir: &Path) -> bool {
    if dir.join(MARKER_NAME).exists() {
        return false;
    }
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// A path component from arbitrary text: anything that is not a letter, a digit, a dash, or a dot becomes a dash, so a version string with a space or a slash in it cannot leave the layout.
fn sanitize(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_owned()
    } else {
        cleaned
    }
}
