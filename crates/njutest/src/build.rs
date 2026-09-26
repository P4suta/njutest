// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Building the workspace's tests, in one of two flavours.

#[cfg(feature = "testkit")]
use std::collections::BTreeMap;
use std::ffi::OsString;
#[cfg(feature = "testkit")]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(feature = "testkit")]
use rust_mutants::cargo::units_of;
use rust_mutants::cargo::{CargoError, Message, Toolchain, parse_messages};
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
    /// Whether the default features stay on.
    /// `false` is `--no-default-features`, written positively so a reader of a configuration is not counting negations.
    pub default_features: bool,
    /// The target ids `[execution] skip_targets` names, carried here so a command that reads the configuration cannot answer about the run without them.
    pub skip_targets: Vec<String>,
}

impl Default for Selection {
    /// The whole workspace, as cargo would build it.
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            features: Vec::new(),
            all_features: false,
            default_features: true,
            skip_targets: Vec::new(),
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
    pub env: rust_mutants::vars::Variables,
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
    #[cfg(feature = "testkit")]
    pub env: rust_mutants::vars::Variables,
    /// What the compiler said, when it refused.
    /// A workspace that does not compile is a finding, not an error.
    pub failure: Option<String>,
    /// What this build could not honour, by name.
    #[cfg(feature = "testkit")]
    pub limitations: Vec<String>,
    /// The files each package's library is made of, workspace-relative, which is the coverage a documented example carries.
    #[cfg(feature = "testkit")]
    pub library_sources: BTreeMap<String, Vec<PathBuf>>,
}

/// Why a build could not be attempted or read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// Cargo's configured compiler flags could not be represented exactly.
    #[error(transparent)]
    Config(#[from] rust_mutants::cargo::config::ConfigError),
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
    /// Cargo printed bytes that are not valid UTF-8 where text is required.
    #[error("{}: {context} is not valid UTF-8: {source}", error::BUILD_UNREADABLE.code)]
    OutputEncoding {
        /// Which output was being decoded.
        context: &'static str,
        /// Why the bytes are not UTF-8.
        #[source]
        source: std::str::Utf8Error,
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
            Self::Config(_) | Self::NotRun { .. } => error::BUILD_FAILED,
            Self::Unreadable { .. } | Self::OutputEncoding { .. } | Self::Protocol { .. } => {
                error::BUILD_UNREADABLE
            }
        }
    }
}

/// Builds the workspace's tests without running them.
///
/// # Errors
/// [`BuildError::NotRun`] when cargo could not be started, and [`BuildError::Unreadable`] when what it printed is not the message stream this version understands.
pub fn build(
    toolchain: &Toolchain,
    packages: &[rust_mutants::cargo::Package],
    options: &BuildOptions,
    watch: Watch<'_>,
) -> Result<Built, BuildError> {
    let mut limitations = Vec::new();
    let mut spec = toolchain.command(&options.root, arguments(toolchain, options));
    let built_with = environment(options, &mut limitations)?;
    spec.env = Some(built_with);
    spec.timeout = options.timeout;
    spec.structured_stdout = Some(BUILD_OUTPUT_LIMIT);

    let built = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &built));
    if let Some(refusal) = never_ran(&built, options.timeout)? {
        return Err(refusal);
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
    if *success != (built.conventional_exit_code() == 0) {
        return Err(BuildError::Protocol {
            message: format!(
                "build-finished says success={success}, but cargo exited with {}",
                built.conventional_exit_code()
            ),
        });
    }
    let units = targets_of(&messages, packages, Some(&options.target_dir))
        .map_err(|error| BuildError::Protocol {
            message: error.to_string(),
        })?
        .into_iter()
        .map(|target| {
            let mut env = spec.env.clone().unwrap_or_default();
            env.overlay(&target.cargo_env);
            env.set("CARGO", toolchain.cargo().as_os_str());
            Unit {
                package: target.package,
                kind: UnitKind::of(target.kind),
                name: target.name,
                harness: target.harness,
                executable: target.executable,
                cwd: target.cwd,
                env,
            }
        })
        .collect();
    #[cfg(feature = "testkit")]
    let library_sources = library_sources(&messages, packages, &options.root)
        .map_err(|source| BuildError::Unreadable { source })?;
    Ok(Built {
        units,
        #[cfg(feature = "testkit")]
        library_sources,
        #[cfg(feature = "testkit")]
        env: spec.env.take().unwrap_or_default(),
        failure: failure_of(&messages, &built.output)?,
        #[cfg(feature = "testkit")]
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

/// Where the profiles an instrumented build script writes go.
///
/// They are the build's, not any test's, and no target's coverage is ever merged from them; naming a directory inside the scratch keeps them out of the tree under verification, which would otherwise be a different tree after every run.
pub const BUILD_PROFILES: &str = "build-profiles";

/// What a cargo configuration costs an instrumented build, which is what the build says it could not honour.
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
fn environment(
    options: &BuildOptions,
    limitations: &mut Vec<String>,
) -> Result<rust_mutants::vars::Variables, BuildError> {
    let mut env = options.env.clone();
    env.set(
        "CARGO_TARGET_DIR",
        options.scratch_build_dir.clone().into_os_string(),
    );
    if options.flavour == Flavour::Native {
        return Ok(env);
    }
    let configured = rustflags::configured(&options.root, &options.env);
    limitations.extend(
        configured_limitations(&configured)
            .into_iter()
            .map(str::to_owned),
    );
    if let Some(flags) = rustflags::encoded(&options.env, &configured, &[COVERAGE_FLAG])? {
        env.set("CARGO_ENCODED_RUSTFLAGS", flags);
        env.remove("RUSTFLAGS");
    }
    env.set(
        crate::coverage::PROFILE_ENV,
        options
            .scratch_build_dir
            .join(BUILD_PROFILES)
            .join("%p-%m.profraw")
            .into_os_string(),
    );
    Ok(env)
}

/// What the compiler said when it refused, or nothing when it did not.
fn failure_of(messages: &[Message], output: &[u8]) -> Result<Option<String>, BuildError> {
    let finished_badly = messages
        .iter()
        .any(|message| matches!(message, Message::BuildFinished { success: false }));
    if !finished_badly {
        return Ok(None);
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
        let output = std::str::from_utf8(output).map_err(|source| BuildError::OutputEncoding {
            context: "cargo's diagnostic output",
            source,
        })?;
        let output = output.trim();
        return Ok(Some(if output.is_empty() {
            "cargo reported an unsuccessful build without a diagnostic".to_owned()
        } else {
            output.to_owned()
        }));
    }
    Ok(Some(rendered.join("\n")))
}

/// Why cargo produced nothing to read, when it did not.
fn never_ran(
    built: &rust_mutants::runner::RunResult,
    timeout: Option<Duration>,
) -> Result<Option<BuildError>, BuildError> {
    let said = if let Some(error) = built.error() {
        error.to_string()
    } else if built.timed_out() {
        format!(
            "cargo did not finish within {} milliseconds",
            timeout.unwrap_or_default().as_millis()
        )
    } else if built.conventional_exit_code() == EXIT_CODE_UNAVAILABLE {
        String::from("cargo was stopped before it reported an exit status")
    } else {
        return Ok(None);
    };
    Ok(Some(BuildError::NotRun {
        message: with_output(&said, &built.output)?,
    }))
}

/// What went wrong, with what cargo said about it.
///
/// The remedy on this code is to run the same cargo command and read what it says — but the run composed an environment of its own, built into a directory it then removed, and holds the bytes cargo wrote.
/// Telling somebody to reproduce output the tool already has is asking them to rebuild a command they cannot see.
fn with_output(said: &str, output: &[u8]) -> Result<String, BuildError> {
    let printed = std::str::from_utf8(output).map_err(|source| BuildError::OutputEncoding {
        context: "cargo's process output",
        source,
    })?;
    let printed = printed.trim();
    if printed.is_empty() {
        return Ok(said.to_owned());
    }
    Ok(format!("{said}; cargo said:\n{printed}"))
}

/// The files each package's library compiles, workspace-relative with forward slashes.
#[cfg(feature = "testkit")]
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
            let relative = match source.strip_prefix(root) {
                Ok(relative) => relative,
                Err(_outside_the_test_workspace) => continue,
            };
            files.push(relative.to_path_buf());
        }
        files.sort();
        files.dedup();
    }
    Ok(found)
}
