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
    /// Every layer, in declaration order.
    pub const ALL: [Self; 3] = [Self::Native, Self::Coverage, Self::Mutants];

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

/// The file cargo takes while it is building. A layer whose lock is held is one a build is using, and no file in it may be removed.
pub const CARGO_LOCK: &str = ".cargo-lock";

/// The directories inside a layer whose files a collection may remove. Everything cargo keeps outside them — the lock, this program's marker — is left alone.
pub const COLLECTABLE: [&str; 5] = ["deps", "build", "incremental", ".fingerprint", "examples"];

/// What a collection did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Swept {
    /// How many bytes every layer held before the collection.
    pub before: u64,
    /// How many bytes went away.
    pub removed: u64,
    /// How many files went away.
    pub files: u32,
    /// The layers a build was using, which nothing was removed from.
    pub busy: Vec<PathBuf>,
}

impl BuildCache {
    /// How many bytes every layer holds, whichever compiler filled it and whether or not a build is using it. The cache is one thing to collect, not one per toolchain.
    ///
    /// # Errors
    /// Never: a layer that cannot be measured contributes nothing, because
    /// this number is for a person reading a line rather than for a decision
    /// that must be exact.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.layers().iter().map(|dir| size_of(dir)).sum()
    }

    /// Removes the least recently used artifacts until every layer together holds at most `max_bytes`.
    ///
    /// Files are removed oldest first, the way `cargo-sweep` does it: cargo
    /// rebuilds whatever it misses, so the cost of removing too much is time
    /// and never correctness. A layer whose `.cargo-lock` is held is one a
    /// build is using, and nothing is removed from it.
    #[must_use]
    pub fn collect(&self, max_bytes: u64) -> Swept {
        let mut swept = Swept::default();
        let mut artifacts: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
        for dir in self.layers() {
            swept.before = swept.before.saturating_add(size_of(&dir));
            if is_busy(&dir) {
                swept.busy.push(dir);
                continue;
            }
            artifacts.extend(collectable(&dir));
        }
        if swept.before <= max_bytes {
            return swept;
        }
        artifacts.sort();
        let mut total = swept.before;
        for (_modified, bytes, path) in artifacts {
            if total <= max_bytes {
                break;
            }
            if fs::remove_file(&path).is_err() {
                continue;
            }
            total = total.saturating_sub(bytes);
            swept.removed = swept.removed.saturating_add(bytes);
            swept.files = swept.files.saturating_add(1);
        }
        swept
    }

    /// Every layer directory that exists, for every compiler that has filled one.
    fn layers(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for layer in Layer::ALL {
            let Ok(entries) = fs::read_dir(self.root.join(layer.dir_name())) else {
                continue;
            };
            found.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir()),
            );
        }
        found.sort();
        found
    }
}

/// Whether a build is using this layer right now.
fn is_busy(dir: &Path) -> bool {
    let lock = dir.join(CARGO_LOCK);
    if !lock.exists() {
        return false;
    }
    match rust_mutants::tempowner::acquire(&lock) {
        Ok(Some(mut held)) => {
            drop(held.release());
            false
        }
        Ok(None) => true,
        Err(_unreadable) => true,
    }
}

/// Every file a collection may remove from one layer, with when it was last used and how big it is.
fn collectable(dir: &Path) -> Vec<(std::time::SystemTime, u64, PathBuf)> {
    let mut found = Vec::new();
    let Ok(profiles) = fs::read_dir(dir) else {
        return found;
    };
    for profile in profiles.flatten() {
        for name in COLLECTABLE {
            let mut pending = vec![profile.path().join(name)];
            while let Some(current) = pending.pop() {
                let Ok(entries) = fs::read_dir(&current) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    if kind.is_dir() {
                        pending.push(entry.path());
                    } else if kind.is_file()
                        && let Ok(metadata) = entry.metadata()
                    {
                        found.push((
                            metadata
                                .modified()
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                            metadata.len(),
                            entry.path(),
                        ));
                    }
                }
            }
        }
    }
    found
}

/// Every regular file under `dir`, added up. Best effort: the number is for a person reading a line.
fn size_of(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}
