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
    /// The arguments this kind adds after `cargo`.
    const fn args(self) -> &'static [&'static str] {
        match self {
            Self::Check => &["check", "--workspace", "--all-targets"],
            Self::Tests => &["test", "--workspace", "--all-targets", "--no-run"],
        }
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
    let mut args: Vec<String> = options
        .kind
        .args()
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
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
    let mut spec = driver.toolchain.command(driver.dir, args);
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
