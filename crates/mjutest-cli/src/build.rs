// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Building the workspace's tests, in one of two flavours.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::cargo::{CargoError, Message, Toolchain, parse_messages, units_of};
use rust_mutants::execute::targets_of;
use rust_mutants::runner::{EXIT_CODE_UNAVAILABLE, run};

use crate::error::{self, ErrorCode};
use crate::rustflags::{self, COVERAGE_FLAG};
use crate::targets::{Unit, UnitKind};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// The largest Cargo JSON message stream retained for one test build.
pub const BUILD_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

/// Which of the two builds this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// What the project's tests are, unmodified.
    Native,
    /// The same, instrumented, so a run can see which regions each test reached.
    Coverage,
}

/// What to compile, as the command line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The packages; empty is the whole workspace.
    pub packages: Vec<String>,
    /// `--features`.
    pub features: Vec<String>,
    /// `--all-features`.
    pub all_features: bool,
    /// Whether the default features stay on. `false` is `--no-default-features`, written positively so a reader of a configuration is not counting negations.
    pub default_features: bool,
}

impl Default for Selection {
    /// The whole workspace, as cargo would build it.
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            features: Vec::new(),
            all_features: false,
            default_features: true,
        }
    }
}

/// How every cargo command of a run is bounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cargo {
    /// `--offline`.
    pub offline: bool,
    /// `--locked`.
    pub locked: bool,
}

/// What to build, where to put it, and what to build it with.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// The workspace root, which the command runs in.
    pub root: PathBuf,
    /// What to compile.
    pub selection: Selection,
    /// Which build this is.
    pub flavour: Flavour,
    /// `--target-dir`: the base cache layer for this flavour.
    pub target_dir: PathBuf,
    /// `CARGO_TARGET_DIR` for anything the build itself starts — a build script that spawns cargo lands here rather than in the base layer.
    pub scratch_build_dir: PathBuf,
    /// The environment the command runs with.
    pub env: Vec<(OsString, OsString)>,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// How long the build may take.
    pub timeout: Option<Duration>,
}

/// What a build produced.
#[derive(Debug, Clone)]
pub struct Built {
    /// The test binaries, with the environment their processes run with.
    pub units: Vec<Unit>,
    /// The environment the build ran with, which is what anything compiling against its artifacts has to run with to reuse them.
    pub env: Vec<(OsString, OsString)>,
    /// What the compiler said, when it refused. A workspace that does not compile is a finding, not an error.
    pub failure: Option<String>,
    /// What this build could not honour, by name.
    pub limitations: Vec<String>,
    /// The files each package's library is made of, workspace-relative, which is the coverage a documented example carries.
    pub library_sources: BTreeMap<String, Vec<PathBuf>>,
}

/// Why a build could not be attempted or read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// cargo could not be started, or was stopped.
    #[error("{}: the build could not be run: {message}", error::BUILD_FAILED.code)]
    NotRun {
        /// What happened.
        message: String,
    },
    /// cargo ran and said something this version cannot read.
    #[error("{}: the build's output could not be read: {source}", error::BUILD_UNREADABLE.code)]
    Unreadable {
        /// The failure.
        #[source]
        source: CargoError,
    },
    /// Cargo's completion record is absent, repeated, or contradicts its exit status.
    #[error("{}: the build's output could not be read: {message}", error::BUILD_UNREADABLE.code)]
    Protocol {
        /// What made the stream incomplete or contradictory.
        message: String,
    },
}

impl BuildError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::NotRun { .. } => error::BUILD_FAILED,
            Self::Unreadable { .. } | Self::Protocol { .. } => error::BUILD_UNREADABLE,
        }
    }
}

/// Builds the workspace's tests without running them.
///
/// # Errors
/// [`BuildError::NotRun`] when cargo could not be started, and
/// [`BuildError::Unreadable`] when what it printed is not the message stream
/// this version understands.
pub fn build(
    toolchain: &Toolchain,
    packages: &[rust_mutants::cargo::Package],
    options: &BuildOptions,
    watch: Watch<'_>,
) -> Result<Built, BuildError> {
    let mut limitations = Vec::new();
    let mut spec = toolchain.command(&options.root, arguments(toolchain, options));
    let built_with = environment(options, &mut limitations);
    spec.env = Some(built_with.clone());
    spec.timeout = options.timeout;
    spec.structured_stdout = Some(BUILD_OUTPUT_LIMIT);

    let built = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &built));
    if let Some(error) = &built.error {
        return Err(BuildError::NotRun {
            message: error.to_string(),
        });
    }
    if built.timed_out {
        return Err(BuildError::NotRun {
            message: format!(
                "cargo did not finish within {} milliseconds",
                options.timeout.unwrap_or_default().as_millis()
            ),
        });
    }
    if built.exit_code == EXIT_CODE_UNAVAILABLE {
        return Err(BuildError::NotRun {
            message: "cargo was stopped before it reported an exit status".to_owned(),
        });
    }
    let messages =
        parse_messages(&built.stdout).map_err(|source| BuildError::Unreadable { source })?;
    let finished: Vec<bool> = messages
        .iter()
        .filter_map(|message| match message {
            Message::BuildFinished { success } => Some(*success),
            _ => None,
        })
        .collect();
    let [success] = finished.as_slice() else {
        return Err(BuildError::Protocol {
            message: format!(
                "the message stream has {} build-finished records instead of one",
                finished.len()
            ),
        });
    };
    if *success != (built.exit_code == 0) {
        return Err(BuildError::Protocol {
            message: format!(
                "build-finished says success={success}, but cargo exited with {}",
                built.exit_code
            ),
        });
    }
    let units = targets_of(&messages, packages, Some(&options.target_dir))
        .into_iter()
        .map(|target| {
            let mut env = spec.env.clone().unwrap_or_default();
            env.retain(|(key, _)| !target.cargo_env.iter().any(|(name, _)| name == key));
            env.extend(target.cargo_env.iter().cloned());
            set(&mut env, "CARGO", toolchain.cargo().as_os_str().to_owned());
            Unit {
                package: target.package,
                kind: UnitKind::of(target.kind),
                name: target.name,
                executable: target.executable,
                cwd: target.cwd,
                env,
            }
        })
        .collect();
    let library_sources = library_sources(&messages, packages, &options.root)
        .map_err(|source| BuildError::Unreadable { source })?;
    Ok(Built {
        units,
        library_sources,
        env: built_with,
        failure: failure_of(&messages, &built.output),
        limitations,
    })
}

/// The command line, in a fixed order so two runs of the same request are the same command.
fn arguments(toolchain: &Toolchain, options: &BuildOptions) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec![
        "test".into(),
        "--no-run".into(),
        "--message-format=json".into(),
    ];
    if options.flavour == Flavour::Coverage {
        arguments.push("--target".into());
        arguments.push(toolchain.host().into());
    }
    if options.cargo.locked {
        arguments.push("--locked".into());
    }
    if options.cargo.offline {
        arguments.push("--offline".into());
    }
    let selection = &options.selection;
    if selection.packages.is_empty() {
        arguments.push("--workspace".into());
    } else {
        for package in &selection.packages {
            arguments.push("--package".into());
            arguments.push(package.into());
        }
    }
    if selection.all_features {
        arguments.push("--all-features".into());
    }
    if !selection.default_features {
        arguments.push("--no-default-features".into());
    }
    if !selection.features.is_empty() {
        arguments.push("--features".into());
        arguments.push(selection.features.join(",").into());
    }
    arguments.push("--target-dir".into());
    arguments.push(options.target_dir.clone().into_os_string());
    arguments
}

/// Where the profiles an instrumented build script writes go. They are the build's, not any test's, and no target's coverage is ever merged from them; naming a directory inside the scratch keeps them out of the tree under verification, which would otherwise be a different tree after every run.
pub const BUILD_PROFILES: &str = "build-profiles";

/// What a cargo configuration costs an instrumented build, which is what the build says it could not honour.
///
/// An instrumented build compiles with a flag of its own, which it can only
/// add by writing every flag the project configured back in one place. Two
/// kinds of flag do not survive that. A `target.*` table says flags whose
/// application is cargo's decision about the target being built rather than
/// this build's, so they are left out. A file this release could not read
/// faithfully says nothing about what it asks for, so nothing of it is written
/// back.
///
/// Both are stated when both apply: a reader told only one of them would go
/// looking for the wrong file, and the coverage a run routes by was taken from
/// a build that differs from the project's in both ways rather than in one.
#[must_use]
pub fn configured_limitations(configured: &rustflags::Configured) -> Vec<&'static str> {
    let mut named = Vec::new();
    if configured.target_specific {
        named.push(crate::limitation::TARGET_RUSTFLAGS_NOT_MERGED);
    }
    if configured.unreadable {
        named.push(rust_mutants::limitation::CARGO_CONFIGURATION_UNREADABLE);
    }
    named
}

/// The environment the build runs with: the run's own, the scratch layer for anything it starts, and the flags the flavour needs.
fn environment(options: &BuildOptions, limitations: &mut Vec<String>) -> Vec<(OsString, OsString)> {
    let mut env = options.env.clone();
    set(
        &mut env,
        "CARGO_TARGET_DIR",
        options.scratch_build_dir.clone().into_os_string(),
    );
    if options.flavour == Flavour::Native {
        return env;
    }
    let configured = rustflags::configured(&options.root, &options.env);
    limitations.extend(
        configured_limitations(&configured)
            .into_iter()
            .map(str::to_owned),
    );
    if let Some(flags) = rustflags::encoded(&options.env, &configured, &[COVERAGE_FLAG]) {
        set(&mut env, "CARGO_ENCODED_RUSTFLAGS", flags);
        env.retain(|(key, _)| key != OsStr::new("RUSTFLAGS"));
    }
    set(
        &mut env,
        crate::coverage::PROFILE_ENV,
        options
            .scratch_build_dir
            .join(BUILD_PROFILES)
            .join("%p-%m.profraw")
            .into_os_string(),
    );
    env
}

/// Sets one variable, replacing what was there.
fn set(env: &mut Vec<(OsString, OsString)>, name: &str, value: OsString) {
    env.retain(|(key, _)| key != OsStr::new(name));
    env.push((OsString::from(name), value));
}

/// What the compiler said when it refused, or nothing when it did not.
fn failure_of(messages: &[Message], output: &[u8]) -> Option<String> {
    let finished_badly = messages
        .iter()
        .any(|message| matches!(message, Message::BuildFinished { success: false }));
    if !finished_badly {
        return None;
    }
    let rendered: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            Message::CompilerMessage(compiler) if compiler.message.is_error() => {
                let rendered = compiler
                    .message
                    .rendered
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or(&compiler.message.message);
                Some(rendered.trim().to_owned())
            }
            _ => None,
        })
        .collect();
    if rendered.is_empty() {
        let output = String::from_utf8_lossy(output);
        let output = output.trim();
        return Some(if output.is_empty() {
            "cargo reported an unsuccessful build without a diagnostic".to_owned()
        } else {
            output.to_owned()
        });
    }
    Some(rendered.join("\n"))
}

/// The files each package's library compiles, workspace-relative with forward slashes.
///
/// A documented example is compiled by rustdoc into a binary this run never
/// sees, so there is no coverage to read for it. What is known is the library
/// it exercises, and these are its files.
fn library_sources(
    messages: &[Message],
    packages: &[rust_mutants::cargo::Package],
    root: &Path,
) -> Result<BTreeMap<String, Vec<PathBuf>>, CargoError> {
    let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for unit in units_of(messages, root)? {
        if !unit.target.is_lib() || unit.target.is_proc_macro() {
            continue;
        }
        let Some(package) = packages
            .iter()
            .find(|package| package.id == unit.package_id)
            .map(|package| package.name.clone())
        else {
            continue;
        };
        let files = found.entry(package).or_default();
        for source in unit.sources {
            if let Ok(relative) = source.strip_prefix(root) {
                files.push(relative.to_path_buf());
            }
        }
        files.sort();
        files.dedup();
    }
    Ok(found)
}
