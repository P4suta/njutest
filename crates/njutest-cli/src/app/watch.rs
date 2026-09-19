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
        if let Entry::File(path) = entry
            && let Ok(held) = std::fs::metadata(&path)
        {
            let _replaced = seen.insert(relative.to_owned(), (held.modified().ok(), held.len()));
        }
        Ok(())
    })?;
    Ok(seen)
}

/// Runs `round` once, then again every time `look` reports the tree has changed, until `cancel`.
pub fn until<L, R>(cancel: &Cancel, poll: Duration, mut look: L, mut round: R) -> u8
where
    L: FnMut() -> Option<Seen>,
    R: FnMut() -> u8,
{
    until_with_wait(cancel, &mut look, &mut round, (poll, std::thread::sleep))
}

pub(crate) fn until_with_wait<L, R, W>(
    cancel: &Cancel,
    mut look: L,
    mut round: R,
    (poll, mut wait): (Duration, W),
) -> u8
where
    L: FnMut() -> Option<Seen>,
    R: FnMut() -> u8,
    W: FnMut(Duration),
{
    let mut last: Option<Seen> = None;
    let mut code = 0;
    while !cancel.is_cancelled() {
        let seen = look();
        if seen.is_none() || seen == last {
            if !cancel.is_cancelled() {
                wait(poll);
            }
            continue;
        }
        code = round();
        last = seen;
    }
    code
}

/// What the round that just finished concluded, when there is a stored report to read back.
///
/// `None` says this round left nothing to compare, which is not the same as
/// a round that found nothing. The difference matters on the first round of
/// all, where an empty stand-in would report every gap in the project as one
/// the reader had just opened ([ADR 0023](../../../docs/adr/0023-a-run-may-not-conclude-from-how-it-measured.md)).
fn concluded(root: &Path) -> Option<crate::presentation::Told> {
    let run = crate::app::runs::resolve(root, None).ok()?;
    let report = crate::app::runs::report(root, &run).ok()?;
    let sources = crate::presentation::Sources::read(root, &report);
    Some(crate::presentation::Told::of(&report, &sources, ""))
}

/// Verifies whenever the tree changes, until the run is interrupted.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = environment.rooted(arguments.verify.directory.as_deref());
    let config = match Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let excluded = Excluded::beside(&config.reports.directory);
    let poll = arguments.poll_ms.map_or(POLL, Duration::from_millis);
    let cancel = environment.cancel.clone();
    super::say(
        stdout,
        &format!("watching\t{}\tevery {}ms", root.display(), poll.as_millis()),
    );
    let mut before: Option<crate::presentation::Told> = None;
    until(
        &cancel,
        poll,
        || look(&root, &excluded).ok(),
        || {
            let code = super::verify::run(&arguments.verify, environment, stdout, stderr);
            if environment.terminal.drawing
                && let Some(now) = concluded(&root)
            {
                if let Some(last) = before.as_ref() {
                    let said = crate::presentation::moved::moved(last, &now, environment.terminal);
                    let _written = stdout.write_all(said.as_bytes());
                }
                before = Some(now);
            }
            super::say(stdout, "waiting\tfor the next change");
            code
        },
    )
}
