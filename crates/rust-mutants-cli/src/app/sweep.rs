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
    /// Whether the store travels out to a file, in from one, or stays.
    pub(super) transport: StoreTransport<'a>,
}

/// Where a store goes, when a command moves it.
#[derive(Debug, Clone, Copy)]
pub(super) enum StoreTransport<'a> {
    /// It stays where it is.
    Stay,
    /// Every record is written to this file.
    Export(&'a Path),
    /// Every record in this file is filed here.
    Import(&'a Path),
}

/// Everything the cache command reports after both collectors have finished.
struct CacheSummary<'a> {
    parent: &'a Path,
    verb: &'a str,
    caches: &'a tempowner::SweepResult,
    snapshots: &'a tempowner::SweepResult,
    records: u32,
    bytes: u64,
    store: &'a Path,
    measurements: &'a str,
}

impl CacheSummary<'_> {
    fn render(&self) -> Result<String, CliError> {
        let failures = self
            .snapshots
            .failures
            .len()
            .checked_add(self.caches.failures.len())
            .ok_or(CliError::ProjectionOverflow {
                projection: "cache",
                field: "the temporary-directory cleanup failure count",
            })?;
        let never_inspected = self
            .snapshots
            .unreached
            .checked_add(self.caches.unreached)
            .ok_or(CliError::ProjectionOverflow {
                projection: "cache",
                field: "the uninspected temporary-directory count",
            })?;
        let mut text = format!(
            "temp         {}\ncaches       {} {}, {} bytes; {} still in use, {} kept for the next run\nsnapshots    {} {}, {} bytes; {} still in use, {} preserved on purpose\noutcomes     {} records, {} bytes, at {}\nmeasurements {}\nfailures     {}\n",
            self.parent.display(),
            self.caches.removed.len(),
            self.verb,
            self.caches.removed_bytes,
            self.caches.live,
            self.caches.cached,
            self.snapshots.removed.len(),
            self.verb,
            self.snapshots.removed_bytes,
            self.snapshots.live,
            self.snapshots.kept,
            self.records,
            self.bytes,
            self.store.display(),
            self.measurements,
            failures,
        );
        for failure in self
            .snapshots
            .failures
            .iter()
            .chain(self.caches.failures.iter())
        {
            let written = writeln!(
                text,
                "             {}: {}",
                failure.dir.display(),
                failure.source
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        text.push_str(&unreached(never_inspected));
        Ok(text)
    }
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
    if let Some(code) = transported(&store, asked.transport, stdout)? {
        return Ok(code);
    }
    if asked.clear_outcomes {
        let (records, bytes) = store
            .clear()
            .map_err(|source| cache_unreadable(store.root(), source))?;
        write(
            stdout,
            &format!(
                "outcomes    {} removed, {bytes} bytes, from {}\n",
                records,
                store.root().display()
            ),
        )?;
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
    let (records, bytes) = store
        .size()
        .map_err(|source| cache_unreadable(store.root(), source))?;
    let measurements = measurements(environment)?;
    let verb = if asked.gc { "removed" } else { "reclaimable" };
    let text = CacheSummary {
        parent,
        verb,
        caches: &taken,
        snapshots: &left,
        records,
        bytes,
        store: store.root(),
        measurements: &measurements,
    }
    .render()?;
    write(stdout, &text)?;
    write(stdout, &preserved(asked, environment)?)?;
    Ok(0)
}

/// Moves the store out to a file or in from one, when asked, and says what moved; nothing when it stays.
fn transported(
    store: &crate::outcomes::Store,
    transport: StoreTransport<'_>,
    stdout: &mut dyn Write,
) -> Result<Option<u8>, CliError> {
    match transport {
        StoreTransport::Stay => Ok(None),
        StoreTransport::Export(file) => {
            let exported = store.export().map_err(rust_mutants::EngineError::from)?;
            let mut text = serde_json::to_string_pretty(&exported)
                .map_err(|source| CliError::writing(file, std::io::Error::other(source)))?;
            text.push('\n');
            rust_mutants::replace::file(file, text.as_bytes())
                .map_err(|failure| CliError::writing(&failure.path, failure.source))?;
            write(
                stdout,
                &format!(
                    "exported    {} records from {} to {}\n",
                    exported.records.len(),
                    store.root().display(),
                    file.display()
                ),
            )?;
            Ok(Some(0))
        }
        StoreTransport::Import(file) => {
            let text =
                std::fs::read_to_string(file).map_err(|source| CliError::CacheUnreadable {
                    path: file.to_path_buf(),
                    source,
                })?;
            let exported: crate::outcomes::Exported = crate::strictjson::decode_str(&text)
                .map_err(|error| CliError::CacheUnreadable {
                    path: file.to_path_buf(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
                })?;
            let filed = store
                .import(&exported)
                .map_err(rust_mutants::EngineError::from)?;
            write(
                stdout,
                &format!(
                    "imported    {filed} records from {} into {}\n",
                    file.display(),
                    store.root().display()
                ),
            )?;
            Ok(Some(0))
        }
    }
}

fn cache_unreadable(path: &Path, source: std::io::Error) -> CliError {
    CliError::CacheUnreadable {
        path: path.to_path_buf(),
        source,
    }
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
fn measurements(environment: &Environment) -> Result<String, CliError> {
    let directory = environment
        .cache_directory
        .join(rust_mutants::reach::remembered::LAYOUT);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!("none yet, at {}", directory.display()));
        }
        Err(source) => {
            return Err(CliError::CacheUnreadable {
                path: directory,
                source,
            });
        }
    };
    let (mut held, mut bytes) = (0_u64, 0_u64);
    for entry in entries {
        let entry = entry.map_err(|source| CliError::CacheUnreadable {
            path: directory.clone(),
            source,
        })?;
        if entry.path().extension().is_some_and(|one| one == "json") {
            held = held.checked_add(1).ok_or(CliError::ProjectionOverflow {
                projection: "cache",
                field: "the remembered-measurement record count",
            })?;
            let metadata = entry
                .metadata()
                .map_err(|source| CliError::CacheUnreadable {
                    path: entry.path(),
                    source,
                })?;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or(CliError::ProjectionOverflow {
                    projection: "cache",
                    field: "the remembered-measurement byte count",
                })?;
        }
    }
    Ok(format!(
        "{held} records, {bytes} bytes, at {}",
        directory.display()
    ))
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
    let ledger =
        crate::kept::Ledger::read(&directory).map_err(|source| CliError::KeptLedgerUnreadable {
            path: directory.join(crate::kept::FILE_NAME),
            source,
        })?;
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
