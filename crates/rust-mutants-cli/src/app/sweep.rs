// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the engine left in the temporary directory, and what a sweep may take back.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use jiff::Timestamp;
use rust_mutants::{snapshot, tempowner, workspace};

use super::write;
use crate::Environment;
use crate::error::CliError;

/// What a `cache` command was asked to do.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person sets on the command line, and a switch is a bool \
              wherever it is stored"
)]
pub(super) struct Sweeping<'a> {
    /// The workspace root, whose report directory holds the ledger of what was kept.
    pub(super) root: Option<&'a Path>,
    /// Remove what is abandoned rather than only saying how much there is.
    pub(super) gc: bool,
    /// Remove every build cache no live run has locked, not only the unowned ones.
    pub(super) all: bool,
    /// Remove the directories a run was asked to keep, too.
    pub(super) kept: bool,
    /// Empty the store of what earlier runs established.
    pub(super) clear_outcomes: bool,
    /// Where the store is, when it is not under the user's cache directory.
    pub(super) cache_dir: Option<&'a Path>,
}

pub(super) fn cache(
    asked: &Sweeping<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let parent = &environment.temp_directory;
    let store = crate::outcomes::Store::new(
        asked
            .cache_dir
            .unwrap_or(environment.cache_directory.as_path()),
    );
    if asked.clear_outcomes {
        let (records, bytes) = store.clear();
        write(
            stdout,
            &format!(
                "outcomes    {} removed, {bytes} bytes, from {}\n",
                records,
                store.root().display()
            ),
        );
        return Ok(0);
    }
    let now = Timestamp::now();
    let scratch = [snapshot::DIR_PREFIX, workspace::SCRATCH_DIR_PREFIX];
    let caches = [workspace::TARGET_DIR_PREFIX];
    let nothing = |_dir: &Path| Ok(());
    let (left, taken) = match (asked.gc, asked.all) {
        (true, true) => (
            tempowner::sweep(parent, &scratch, now),
            tempowner::reclaim(parent, &caches, now),
        ),
        (true, false) => (
            tempowner::sweep(parent, &scratch, now),
            tempowner::sweep(parent, &caches, now),
        ),
        (false, _) => (
            tempowner::sweep_with(parent, &scratch, now, &nothing),
            tempowner::reclaim_with(parent, &caches, now, &nothing),
        ),
    };
    let left = left.map_err(|source| CliError::writing(parent, source))?;
    let taken = taken.map_err(|source| CliError::writing(parent, source))?;
    let (records, bytes) = store.size();
    let mut text = String::new();
    let verb = if asked.gc { "removed" } else { "reclaimable" };
    let written = write!(
        text,
        "temp         {}\ncaches       {} {}, {} bytes; {} still in use, {} kept for the next run\nsnapshots    {} {}, {} bytes; {} still in use, {} preserved on purpose\noutcomes     {} records, {} bytes, at {}\nmeasurements {}\nfailures     {}\n",
        parent.display(),
        taken.removed.len(),
        verb,
        taken.removed_bytes,
        taken.live,
        taken.cached,
        left.removed.len(),
        verb,
        left.removed_bytes,
        left.live,
        left.kept,
        records,
        bytes,
        store.root().display(),
        measurements(environment),
        left.failures.len().saturating_add(taken.failures.len()),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    for failure in left.failures.iter().chain(taken.failures.iter()) {
        let written = writeln!(
            text,
            "             {}: {}",
            failure.dir.display(),
            failure.source
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push_str(&unreached(left.unreached.saturating_add(taken.unreached)));
    write(stdout, &text);
    write(stdout, &preserved(asked, environment)?);
    Ok(0)
}

/// What a sweep that spent its budget says, which is what it did not look at rather than what it failed to remove.
fn unreached(count: usize) -> String {
    if count == 0 {
        return String::new();
    }
    format!(
        "unreached    {count} left for the next sweep; it spent its {} seconds on the ones \
         before them\n             try: something is holding a directory open, and a sweep \
         cannot take it back\n",
        tempowner::SWEEP_BUDGET.as_secs()
    )
}

/// What measuring trees established, which a run of an unchanged tree reads instead of measuring again.
fn measurements(environment: &Environment) -> String {
    let directory = environment
        .cache_directory
        .join(rust_mutants::reach::remembered::LAYOUT);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return format!("none yet, at {}", directory.display());
    };
    let (mut held, mut bytes) = (0_u64, 0_u64);
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|one| one == "json") {
            held = held.saturating_add(1);
            bytes = bytes.saturating_add(entry.metadata().map_or(0, |it| it.len()));
        }
    }
    format!("{held} records, {bytes} bytes, at {}", directory.display())
}

/// The directories runs were asked to keep, listed or removed.
fn preserved(asked: &Sweeping<'_>, environment: &Environment) -> Result<String, CliError> {
    let root = environment.rooted(asked.root);
    let directory = super::stored::Store::read(&root).root();
    if asked.kept {
        let (removed, left) = crate::kept::Ledger::clear(&directory)
            .map_err(|source| CliError::writing(&directory, source))?;
        let mut text = format!("kept         {removed} removed\n");
        for entry in &left.kept {
            let written = writeln!(
                text,
                "             still there: {} ({})",
                entry.path.display(),
                entry.run_id
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        if !left.kept.is_empty() {
            text.push_str(
                "             try: something is holding these open, and a sweep cannot take \
                 them back\n",
            );
        }
        return Ok(text);
    }
    let ledger = crate::kept::Ledger::read(&directory);
    let mut text = format!("kept         {}\n", ledger.kept.len());
    for entry in &ledger.kept {
        let written = writeln!(
            text,
            "             {} ({})",
            entry.path.display(),
            entry.run_id
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    Ok(text)
}
