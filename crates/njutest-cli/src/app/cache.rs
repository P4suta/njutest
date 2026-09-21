// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest cache`: what earlier runs left behind, and collecting what is no longer an answer.

use std::io::Write;
use std::path::Path;

use jiff::Timestamp;

use crate::cache::store::Store;
use crate::cli::{Cache, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::config::Config;

/// Says what the store holds, and collects it when asked.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Cache,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = environment.rooted(arguments.directory.as_deref());
    let config = match Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let store = Store::new(
        &environment.cache_directory,
        config.cache.max_bytes,
        config.cache.ttl,
    );
    if let Some(destination) = arguments.export.as_ref() {
        return said(carry(&store, destination, Direction::Out), stdout, stderr);
    }
    if let Some(source) = arguments.import.as_ref() {
        return said(carry(&store, source, Direction::In), stdout, stderr);
    }
    let collected = if arguments.gc {
        match store.collect(Timestamp::now()) {
            Ok(collected) => collected,
            Err(error) => {
                super::complain(stderr, &error, error.code())?;
                return Ok(EXIT_ERROR);
            }
        }
    } else {
        crate::cache::store::Collected::default()
    };
    let status = match store.status() {
        Ok(status) => status,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    super::say(stdout, &format!("root      {}", store.root().display()))?;
    super::say(
        stdout,
        &format!(
            "holds     {} answers, {} bytes of {}",
            status.entries,
            status.bytes,
            if config.cache.max_bytes == 0 {
                String::from("no bound")
            } else {
                format!("at most {}", config.cache.max_bytes)
            }
        ),
    )?;
    super::say(
        stdout,
        &if arguments.gc {
            format!(
                "collected {} expired, {} evicted, {} bytes",
                collected.expired.len(),
                collected.evicted.len(),
                collected.bytes
            )
        } else {
            String::from("collected nothing; --gc is what collects")
        },
    )?;
    temporary(environment, arguments.gc, stdout)?;
    if let Err(error) = preserved(&root, stdout) {
        super::complain(stderr, &error, crate::error::CACHE_CORRUPT)?;
        return Ok(EXIT_ERROR);
    }
    Ok(EXIT_ASSURED)
}

/// Which way answers are moving between this machine and a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Out of the store, into the file.
    Out,
    /// Out of the file, into the store.
    In,
}

/// Carries answers between this machine's store and a file, and says how many moved.
fn carry(
    store: &Store,
    path: &Path,
    direction: Direction,
) -> Result<String, crate::cache::store::CacheError> {
    let opening = |source| crate::cache::store::CacheError::Unusable {
        path: path.to_path_buf(),
        source,
    };
    let (said, moved) = match direction {
        Direction::Out => (
            "exported",
            std::fs::File::create(path)
                .map_err(opening)
                .and_then(|file| store.export(&mut std::io::BufWriter::new(file)))?,
        ),
        Direction::In => (
            "imported",
            std::fs::File::open(path)
                .map_err(opening)
                .and_then(|mut file| store.import(&mut file))?,
        ),
    };
    Ok(format!("{said}  {moved} answers\t{}", path.display()))
}

/// Says what a carrying came to, or diagnoses why it did not happen.
fn said(
    carried: Result<String, crate::cache::store::CacheError>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    match carried {
        Ok(line) => {
            super::say(stdout, &line)?;
            Ok(EXIT_ASSURED)
        }
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            Ok(EXIT_ERROR)
        }
    }
}

/// What runs preserved on purpose and is still there. The ledger names a directory; the directory's own marker says whether it may be removed, so this reports rather than collects.
fn preserved(root: &Path, stdout: &mut dyn Write) -> std::io::Result<()> {
    let ledger = crate::kept::read(root)?;
    let left = crate::kept::forget_gone(&ledger);
    super::say(
        stdout,
        &format!("kept      {} directories", left.kept.len()),
    )?;
    for one in &left.kept {
        super::say(stdout, &format!("          {} ({})", one.path, one.run_id))?;
    }
    Ok(())
}

/// What earlier runs left in the operating system's temporary directory, collected when asked.
///
/// Reporting an abandoned directory and leaving it there tells a reader they
/// have rubbish and gives them nothing to sweep it with — and `--gc` is the
/// flag they would reach for. It collects here too, and without it the line
/// says what would go rather than what went.
fn temporary(
    environment: &Environment,
    collect: bool,
    stdout: &mut dyn Write,
) -> std::io::Result<()> {
    let nothing = |_dir: &Path| Ok(());
    let remove = |dir: &Path| std::fs::remove_dir_all(dir);
    let swept = rust_mutants::tempowner::sweep_with(
        &environment.temp_directory,
        &[
            rust_mutants::snapshot::DIR_PREFIX,
            crate::scratch::DIR_PREFIX,
        ],
        Timestamp::now(),
        if collect { &remove } else { &nothing },
    )?;
    super::say(
        stdout,
        &format!(
            "temp      {}: {} {}, {} in use, {} preserved on purpose",
            environment.temp_directory.display(),
            swept.removed.len(),
            if collect { "removed" } else { "collectable" },
            swept.live,
            swept.kept
        ),
    )?;
    if swept.unreached > 0 {
        super::say(
            stdout,
            &format!(
                "          {} left for the next sweep; something is holding a directory \
                 open, and a sweep cannot take it back",
                swept.unreached
            ),
        )?;
    }
    Ok(())
}
