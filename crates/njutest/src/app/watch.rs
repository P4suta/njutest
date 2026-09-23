// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest watch`: verify again whenever the tree changes, and say what moved.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use rust_mutants::runner::Cancel;

use crate::cli::{EXIT_ERROR, Environment, Watch as Arguments};
use crate::config::Config;
use crate::evidence::tree::{Bounds, Entry, Excluded, ScanError, walk};

/// How often the tree is asked whether it changed, when the caller does not say.
pub const POLL: Duration = Duration::from_millis(500);

/// What one look at the tree found: every file under verification, by when it was last written and how big it is.
pub type Seen = BTreeMap<String, (Option<SystemTime>, u64)>;

/// What one look at the tree found, without reading any of it.
///
/// # Errors
/// See [`ScanError`].
pub fn look(root: &Path, excluded: &Excluded) -> Result<Seen, ScanError> {
    let mut seen = Seen::new();
    let within = Bounds {
        exclude: &[],
        elsewhere: &[],
        excluded,
    };
    walk(root, &within, |relative, entry| {
        if let Entry::File(path) = entry {
            let held = std::fs::metadata(&path).map_err(|source| ScanError::Unreadable {
                path: path.clone(),
                source,
            })?;
            let modified = held.modified().map_err(|source| ScanError::Unreadable {
                path: path.clone(),
                source,
            })?;
            if seen
                .insert(relative.to_owned(), (Some(modified), held.len()))
                .is_some()
            {
                return Err(ScanError::Malformed {
                    path: relative.to_owned(),
                    message: "the tree walker returned the same path twice".to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(seen)
}

/// Runs `round` once, then again every time `look` reports the tree has changed, until `cancel`.
///
/// # Errors
/// Returns the first observation or round failure.
pub fn until<L, R, E>(cancel: &Cancel, poll: Duration, mut look: L, mut round: R) -> Result<u8, E>
where
    L: FnMut() -> Result<Option<Seen>, E>,
    R: FnMut() -> Result<u8, E>,
{
    until_with_wait(cancel, &mut look, &mut round, (poll, std::thread::sleep))
}

pub(crate) fn until_with_wait<L, R, W, E>(
    cancel: &Cancel,
    mut look: L,
    mut round: R,
    (poll, mut wait): (Duration, W),
) -> Result<u8, E>
where
    L: FnMut() -> Result<Option<Seen>, E>,
    R: FnMut() -> Result<u8, E>,
    W: FnMut(Duration),
{
    let mut last: Option<Seen> = None;
    let mut code = 0;
    while !cancel.is_cancelled() {
        let seen = look()?;
        if seen.is_none() || seen == last {
            if !cancel.is_cancelled() {
                wait(poll);
            }
            continue;
        }
        code = round()?;
        last = seen;
    }
    Ok(code)
}

/// What the round that just finished concluded, when there is a stored report to read back.
///
/// `None` says this round left nothing to compare, which is not the same as a round that found nothing.
/// The difference matters on the first round of all, where an empty stand-in would report every gap in the project as one the reader had just opened (ADR 0023).
fn concluded(root: &Path) -> Result<crate::presentation::Told, ConclusionError> {
    let run = crate::app::runs::resolve(root, None)?;
    let report = crate::app::runs::report(&run)?;
    let sources = crate::presentation::Sources::read(root, &report)?;
    Ok(crate::presentation::Told::of(&report, &sources, "")?)
}

/// Why a completed watch round could not be compared with its predecessor.
#[derive(Debug, thiserror::Error)]
enum ConclusionError {
    /// The completed run or its durable report could not be read.
    #[error(transparent)]
    Run(#[from] crate::app::runs::RunError),
    /// The report's exact projection exceeded a durable counter.
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
}

/// A watch failure that must either stop the command or reach its output boundary unchanged.
#[derive(Debug, thiserror::Error)]
enum WatchCommandError {
    /// The watched tree could not be observed completely.
    #[error(transparent)]
    Scan(#[from] ScanError),
    /// A diagnostic or command result could not be written.
    #[error(transparent)]
    Output(#[from] std::io::Error),
}

/// Verifies whenever the tree changes, until the run is interrupted.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = environment.rooted(arguments.verify.directory.as_deref());
    let config = match Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let excluded = match Excluded::beside(config.reports.directory.as_path()) {
        Ok(excluded) => excluded,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let poll = match arguments.poll_ms {
        Some(milliseconds) => Duration::from_millis(milliseconds),
        None => POLL,
    };
    let cancel = environment.cancel.clone();
    super::say(
        stdout,
        &format!("watching\t{}\tevery {}ms", root.display(), poll.as_millis()),
    )?;
    let mut before: Option<crate::presentation::Told> = None;
    let watched = until(
        &cancel,
        poll,
        || {
            look(&root, &excluded)
                .map(Some)
                .map_err(WatchCommandError::from)
        },
        || {
            let code = super::verify::run(&arguments.verify, environment, stdout, stderr)?;
            if environment.terminal.drawing {
                match concluded(&root) {
                    Ok(now) => {
                        if let Some(last) = before.as_ref() {
                            let said =
                                crate::presentation::moved::moved(last, &now, environment.terminal);
                            stdout.write_all(said.as_bytes())?;
                        }
                        before = Some(now);
                    }
                    Err(error) => {
                        super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
                        return Ok(EXIT_ERROR);
                    }
                }
            }
            super::say(stdout, "waiting\tfor the next change")?;
            Ok(code)
        },
    );
    match watched {
        Ok(code) => Ok(code),
        Err(WatchCommandError::Scan(error)) => {
            super::complain(stderr, &error, error.code())?;
            Ok(EXIT_ERROR)
        }
        Err(WatchCommandError::Output(error)) => Err(error),
    }
}
