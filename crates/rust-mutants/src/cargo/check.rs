// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo check --all-targets --message-format=json`: the pristine gate and
//! the source of every unit's file set.

use std::path::PathBuf;
use std::time::Duration;

use super::depinfo::{Unit, units_from_check};
use super::locate::command_failed;
use super::messages::{Message, parse_messages};
use super::{CargoError, CargoErrorKind, Driver};
use crate::runner::run;
use crate::trace::ExecRecord;

/// How much of the message stream is kept.
const MESSAGE_OUTPUT_LIMIT: usize = 256 << 20;

/// Configures [`check`].
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// `--target-dir`. `None` lets cargo choose, which inside a snapshot is
    /// the snapshot's own `target`.
    pub target_dir: Option<PathBuf>,
    /// Pass `--locked`.
    pub locked: bool,
    /// Pass `--offline`.
    pub offline: bool,
    /// How long the check may take.
    pub timeout: Option<Duration>,
}

/// What a check produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Whether every unit compiled.
    pub success: bool,
    /// Every message, in order, for attribution.
    pub messages: Vec<Message>,
    /// The units that produced an artifact, with their sources. A failed
    /// unit produces none, so on a failed check this is partial.
    pub units: Vec<Unit>,
}

/// Runs `cargo check --workspace --all-targets --message-format=json` in the
/// driver's directory and reads what it said.
///
/// A tree that does not compile is not an error here: it is a
/// [`Checked`] with `success == false` and the diagnostics that say why,
/// because the validation phase reads those diagnostics. A cargo that
/// could not run, or a stream that could not be read, is an error.
///
/// # Errors
///
/// [`CargoErrorKind::CommandFailed`] when cargo itself could not run or
/// timed out, [`CargoErrorKind::MessageUnparsable`] for a stream that is
/// not messages, and the dep-info errors of [`units_from_check`].
pub fn check(driver: &Driver<'_>, options: &CheckOptions) -> Result<Checked, CargoError> {
    let mut args: Vec<String> = vec![
        "check".to_owned(),
        "--workspace".to_owned(),
        "--all-targets".to_owned(),
        "--message-format=json".to_owned(),
    ];
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
            "cargo check printed more than the engine keeps",
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
        // cargo exits non-zero on a failed build; a zero exit without a
        // build-finished success is a stream this engine does not
        // understand.
        return Err(CargoError::new(
            CargoErrorKind::MessageUnparsable,
            "cargo check exited 0 without reporting a finished build",
        ));
    }
    let units = units_from_check(&messages, driver.dir)?;
    Ok(Checked {
        success,
        messages,
        units,
    })
}
