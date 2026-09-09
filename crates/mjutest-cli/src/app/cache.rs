// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest cache`: what earlier runs left behind, and collecting what is no longer an answer.

use std::io::Write;
use std::path::Path;

use jiff::Timestamp;

use crate::cache::store::Store;
use crate::cli::{Cache, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::config::Config;

/// Says what the store holds, and collects it when asked.
pub fn run(
    arguments: &Cache,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = arguments
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
                super::diagnose(stderr, &error.to_string());
                return EXIT_ERROR;
            }
        }
    } else {
        crate::cache::store::Collected::default()
    };
    let status = match store.status() {
        Ok(status) => status,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    super::say(stdout, &format!("root      {}", store.root().display()));
    super::say(
        stdout,
        &format!(
            "holds     {} answers, {} bytes of at most {}",
            status.entries, status.bytes, config.cache.max_bytes
        ),
    );
    super::say(
        stdout,
        &format!(
            "collected {} expired, {} evicted, {} bytes",
            collected.expired.len(),
            collected.evicted.len(),
            collected.bytes
        ),
    );
    build_layers(arguments, environment, &config, stdout);
    temporary(environment, stdout);
    preserved(&root, stdout);
    EXIT_ASSURED
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
///
/// A command that carried nothing says so with the same line as one that
/// carried a thousand, because "it worked" and "there was nothing to work on"
/// are different facts and a job that silently exports an empty store is one
/// whose matrix quietly builds everything twice.
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
) -> u8 {
    match carried {
        Ok(line) => {
            super::say(stdout, &line);
            EXIT_ASSURED
        }
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            EXIT_ERROR
        }
    }
}

/// What runs preserved on purpose and is still there. The ledger names a directory; the directory's own marker says whether it may be removed, so this reports rather than collects.
fn preserved(root: &Path, stdout: &mut dyn Write) {
    let left = crate::kept::forget_gone(&crate::kept::read(root));
    super::say(
        stdout,
        &format!("kept      {} directories", left.kept.len()),
    );
    for one in &left.kept {
        super::say(stdout, &format!("          {} ({})", one.path, one.run_id));
    }
}

/// What the machine-wide build cache holds, and what a collection took from it.
fn build_layers(
    arguments: &Cache,
    environment: &Environment,
    config: &Config,
    stdout: &mut dyn Write,
) {
    let build = crate::build_cache::BuildCache::new(
        &environment.cache_directory.join("mjutest"),
        "every-toolchain",
    );
    let swept = if arguments.gc {
        build.collect(config.cache.build_max_bytes)
    } else {
        crate::build_cache::Swept {
            before: build.size(),
            ..crate::build_cache::Swept::default()
        }
    };
    super::say(
        stdout,
        &format!(
            "builds    {} bytes of at most {}",
            swept.before, config.cache.build_max_bytes
        ),
    );
    super::say(
        stdout,
        &format!(
            "collected {} artifacts, {} bytes; {} layers a build is using",
            swept.files,
            swept.removed,
            swept.busy.len()
        ),
    );
}

/// What earlier runs left in the operating system's temporary directory. The engine sweeps its own prefixes; this reports rather than duplicating it.
///
/// A sweep spares a build cache whatever its age, because a cache exists to
/// outlive the run that filled it. So the count of what a sweep took says
/// nothing about what is on the disk, and a report that gives only that number
/// reads as "there is nothing here" while a machine fills up with caches
/// keyed to trees that are gone.
fn temporary(environment: &Environment, stdout: &mut dyn Write) {
    let swept = rust_mutants::tempowner::sweep_with(
        &environment.temp_directory,
        &[
            rust_mutants::snapshot::DIR_PREFIX,
            crate::scratch::DIR_PREFIX,
        ],
        Timestamp::now(),
        &|_dir| Ok(()),
    )
    .unwrap_or_default();
    super::say(
        stdout,
        &format!(
            "temp      {}: {} abandoned, {} in use, {} preserved on purpose, {} spared as \
             a build cache",
            environment.temp_directory.display(),
            swept.removed.len(),
            swept.live,
            swept.kept,
            swept.cached
        ),
    );
}
