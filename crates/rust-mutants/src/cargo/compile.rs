// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiling the tree and reading what the compiler said: the pristine gate, the source of every unit's file set, and the build validation and execution both stand on.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::depinfo::{Unit, units_of};
use super::locate::command_failed;
use super::messages::{Message, parse_messages};
use super::{CargoError, CargoErrorKind, Driver};
use crate::runner::run;
use crate::trace::ExecRecord;

/// How much of the message stream is kept.
const MESSAGE_OUTPUT_LIMIT: usize = 256 << 20;

/// Which command compiles the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompileKind {
    /// `cargo check --all-targets`: every type question, no code generated.
    #[default]
    Check,
    /// `cargo test --all-targets --no-run`: the binaries a run executes, and every refusal that only happens once code is generated.
    Tests,
}

impl CompileKind {
    /// The command this kind runs, and what it asks of every target.
    const fn command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Check => ("check", &["--all-targets"]),
            Self::Tests => ("test", &["--all-targets", "--no-run"]),
        }
    }
}

/// What this compilation is asked to cover, before the flags that are the same either way.
///
/// A check answers "is this edit a program", and a mutation of one package can
/// stop being one only where another instantiates it, so a check is always
/// about the whole workspace. A test build answers "which binaries will this
/// run start", and a run only ever starts the binaries of the packages it is
/// about, so building the rest is work nothing reads. Scoping a run is the one
/// thing a person can do to make it shorter, and it did not use to shorten the
/// longest part of it.
fn arguments(kind: CompileKind, packages: &[String]) -> Vec<String> {
    let (command, rest) = kind.command();
    let mut args = vec![command.to_owned()];
    if packages.is_empty() || kind == CompileKind::Check {
        args.push("--workspace".to_owned());
    } else {
        for package in packages {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
    }
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    args
}

/// What a build is asked to compile, beyond the tree itself.
///
/// Cargo compiles a different program for a different feature set, target
/// triple, or profile, and a run that measures one of them while the project
/// ships another measures a program nobody runs. These are the words a person
/// would have typed, passed on unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildConfig {
    /// The features to turn on, which cargo takes as one comma-separated argument.
    pub features: Vec<String>,
    /// Pass `--all-features`.
    pub all_features: bool,
    /// Pass `--no-default-features`.
    pub no_default_features: bool,
    /// The target triple to compile for. `None` is the host.
    pub target: Option<String>,
    /// The cargo profile to compile with. `None` is the command's own default.
    pub profile: Option<String>,
    /// How many compilation jobs cargo may run at once. `None` lets cargo choose.
    pub jobs: Option<u32>,
    /// Whether the compiler writes debug information into what it builds.
    ///
    /// A run reads what a test harness printed and never a backtrace, so the
    /// debug information a build writes is bytes nobody reads — and on a real
    /// workspace it is most of what a build writes: six gigabytes against one
    /// for this repository's own engine. Every byte of it is generated,
    /// linked, and written to a temporary directory that is thrown away.
    ///
    /// Somebody who wants a debugger on a kept snapshot asks for it, and then
    /// nothing here overrides the profile they wrote.
    pub debug: bool,
}

impl BuildConfig {
    /// Whether this asks for nothing cargo would not have done anyway.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// The arguments that spell this configuration, in cargo's own order.
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.no_default_features {
            args.push("--no-default-features".to_owned());
        }
        if self.all_features {
            args.push("--all-features".to_owned());
        }
        if !self.features.is_empty() {
            args.push("--features".to_owned());
            args.push(self.features.join(","));
        }
        for (flag, value) in [("--target", &self.target), ("--profile", &self.profile)] {
            if let Some(value) = value {
                args.push(flag.to_owned());
                args.push(value.clone());
            }
        }
        if let Some(jobs) = self.jobs {
            args.push("--jobs".to_owned());
            args.push(jobs.to_string());
        }
        args
    }

    /// What tells cargo to write no debug information, when nothing asked for any.
    ///
    /// Only the two profiles this engine drives are named. A profile somebody
    /// chose with `--profile` is one they meant, and editing it would be this
    /// engine deciding something about a build it was told how to make.
    pub(crate) fn without_debug_information(&self) -> Vec<String> {
        if self.debug || self.profile.is_some() {
            return Vec::new();
        }
        ["profile.dev.debug=0", "profile.test.debug=0"]
            .into_iter()
            .flat_map(|setting| ["--config".to_owned(), setting.to_owned()])
            .collect()
    }
}

/// Configures [`compile`].
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Which command to run.
    pub kind: CompileKind,
    /// `--target-dir`. `None` lets cargo choose, which inside a snapshot is the snapshot's own `target`.
    pub target_dir: Option<PathBuf>,
    /// Pass `--locked`.
    pub locked: bool,
    /// Pass `--offline`.
    pub offline: bool,
    /// How long the check may take.
    pub timeout: Option<Duration>,
    /// What this compilation alone adds to the toolchain's environment, such as the flags a coverage build needs.
    pub env: Vec<(OsString, OsString)>,
    /// The member packages this compilation is about. Empty is the whole workspace, and a check is always about the whole workspace whatever this says.
    pub packages: Vec<String>,
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: BuildConfig,
}

/// The whole command line one compilation runs, which is what a person would have typed.
#[must_use]
pub fn compile_arguments(options: &CompileOptions) -> Vec<String> {
    let mut args = arguments(options.kind, &options.packages);
    args.push("--message-format=json".to_owned());
    if options.locked {
        args.push("--locked".to_owned());
    }
    if options.offline {
        args.push("--offline".to_owned());
    }
    if let Some(target_dir) = &options.target_dir {
        args.push("--target-dir".to_owned());
        args.push(target_dir.to_string_lossy().into_owned());
    }
    args.extend(options.build.arguments());
    args.extend(options.build.without_debug_information());
    args
}

/// What a compilation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// Whether every unit compiled.
    pub success: bool,
    /// Every message, in order, for attribution.
    pub messages: Vec<Message>,
    /// The units that produced an artifact, with their sources. A failed unit produces none, so on a failed check this is partial.
    pub units: Vec<Unit>,
}

/// Compiles the tree in the driver's directory and reads what it said.
///
/// # Errors
/// [`CargoErrorKind::CommandFailed`] when cargo itself could not run or
/// timed out, [`CargoErrorKind::MessageUnparsable`] for a stream that is
/// not messages, and the dep-info errors of [`units_of`].
pub fn compile(driver: &Driver<'_>, options: &CompileOptions) -> Result<Compiled, CargoError> {
    let mut spec = driver
        .toolchain
        .command(driver.dir, compile_arguments(options));
    if !options.env.is_empty() {
        let mut env = spec.env.clone().unwrap_or_default();
        env.retain(|(name, _)| !options.env.iter().any(|(other, _)| other == name));
        env.extend(options.env.iter().cloned());
        spec.env = Some(env);
    }
    spec.structured_stdout = Some(MESSAGE_OUTPUT_LIMIT);
    spec.timeout = options.timeout;
    let result = run(&spec, driver.cancel);
    driver.trace.exec(ExecRecord::of(&spec, &result));
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the compilation was cancelled",
        ));
    }
    if result.error.is_some() || result.timed_out {
        return Err(command_failed(&spec, &result));
    }
    if result.stdout_truncated {
        return Err(CargoError::new(
            CargoErrorKind::MessageUnparsable,
            "the compiler printed more than the engine keeps",
        ));
    }
    let messages = parse_messages(&result.stdout)?;
    let success = messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::BuildFinished { success } => Some(*success),
            _ => None,
        })
        .unwrap_or(false);
    if !success && result.ok() {
        return Err(CargoError::new(
            CargoErrorKind::MessageUnparsable,
            "the compiler exited 0 without reporting a finished build",
        ));
    }
    let units = units_of(&messages, driver.dir)?;
    Ok(Compiled {
        success,
        messages,
        units,
    })
}
