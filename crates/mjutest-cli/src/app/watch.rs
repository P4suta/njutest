// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest watch`: verify again whenever the tree changes, and say what moved.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use rust_mutants::runner::Cancel;

use crate::cli::{EXIT_ERROR, Environment, Watch as Arguments};
use crate::config::Config;
use crate::evidence::tree::{Entry, ScanError, walk};

/// How often the tree is asked whether it changed, when the caller does not say.
pub const POLL: Duration = Duration::from_millis(500);

/// What one look at the tree found: every file under verification, by when it was last written and how big it is.
///
/// This is not what decides whether a run may reuse an earlier answer — that
/// is the tree digest, and it reads every byte. This decides only *when* to
/// ask, so it may be as cheap as a directory walk: on this workspace it is
/// 10ms against the digest's 163ms, and a watch loop pays it every half
/// second.
///
/// A write that restores a file's modification time and its length is a change
/// this misses. It is a change the round after it will see, and a missed round
/// is a round that did not happen rather than a claim about a tree that was
/// not read: no verdict rests on this.
pub type Seen = BTreeMap<String, (Option<SystemTime>, u64)>;

/// What one look at the tree found, without reading any of it.
///
/// # Errors
/// See [`ScanError`].
pub fn look(root: &Path, config: &Config) -> Result<Seen, ScanError> {
    let exclude: Vec<rust_mutants::glob::Pattern> = config
        .project
        .exclude
        .iter()
        .filter_map(|pattern| rust_mutants::glob::Pattern::compile(pattern).ok())
        .collect();
    let mut seen = Seen::new();
    walk(root, &exclude, &[], |relative, entry| {
        if let Entry::File(path) = entry {
            let held = std::fs::metadata(&path).ok();
            let _replaced = seen.insert(
                relative.to_owned(),
                (
                    held.as_ref().and_then(|one| one.modified().ok()),
                    held.map_or(0, |one| one.len()),
                ),
            );
        }
        Ok(())
    })?;
    Ok(seen)
}

/// Runs `round` once, then again every time `look` reports the tree has changed, until `cancel`.
///
/// Nothing in here reads the process or the clock beyond sleeping, so a test
/// drives it with a `look` that returns what it likes and a `round` that
/// cancels when it has seen enough.
///
/// The state a round answered for is the state that was read *before* it, not
/// after. An edit that lands while a round is running has not been answered
/// for, and taking the tree as it stands when the round finishes would fold
/// that edit into an answer that never saw it. What a round writes does not
/// enter this at all — `reports`, `.mjutest` and `target` are outside the walk
/// — so there is no round of its own making to absorb.
pub fn until<L, R>(cancel: &Cancel, poll: Duration, mut look: L, mut round: R) -> u8
where
    L: FnMut() -> Option<Seen>,
    R: FnMut() -> u8,
{
    let mut last: Option<Seen> = None;
    let mut code = 0;
    while !cancel.is_cancelled() {
        let seen = look();
        if seen.is_none() || seen == last {
            if !cancel.is_cancelled() {
                std::thread::sleep(poll);
            }
            continue;
        }
        code = round();
        last = seen;
    }
    code
}

/// Verifies whenever the tree changes, until the run is interrupted.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = arguments
        .verify
        .directory
        .clone()
        .unwrap_or_else(|| environment.working_directory.clone());
    let config = match Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let poll = arguments.poll_ms.map_or(POLL, Duration::from_millis);
    let cancel = environment.cancel.clone();
    super::say(
        stdout,
        &format!("watching\t{}\tevery {}ms", root.display(), poll.as_millis()),
    );
    until(
        &cancel,
        poll,
        || look(&root, &config).ok(),
        || {
            let code = super::verify::run(&arguments.verify, environment, stdout, stderr);
            super::say(stdout, "waiting\tfor the next change");
            code
        },
    )
}
