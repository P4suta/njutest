// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The owner of a directory the gate keeps outside every checkout, in the shape `schema/temp-owner-v1.json` publishes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use serde::Serialize;

/// Which program wrote the marker.
pub const SCHEMA: &str = "njutest-gate-temp-owner-v1";

/// The lock the owner holds for as long as it uses the directory.
const LOCK: &str = "owner.lock";

/// The document saying who that is.
const MARKER: &str = "owner.json";

/// The file that says a directory is a cache, which backup and indexing tools and Cargo itself recognise.
const CACHEDIR_TAG: &str = "CACHEDIR.TAG";

/// What [`CACHEDIR_TAG`] holds: the signature the convention requires, then who made it.
const TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# The njutest push gate's build directory; see schema/temp-owner-v1.json.\n";

/// How many times a claim makes the directory again after a collector took it while the claim waited.
const REMADE: usize = 3;

/// The marker's fields, in the published shape.
#[derive(Debug, Serialize)]
struct Marker<'a> {
    schema: &'a str,
    pid: u32,
    started: String,
    kept: bool,
    role: &'a str,
    keyed_to: &'a str,
}

/// A directory this process owns until the answer is dropped.
#[derive(Debug)]
pub struct Owned {
    _lock: File,
}

/// Claims `dir` as a cache for `keyed_to`, waiting out any other holder, and makes it again if a collector took it meanwhile.
///
/// # Errors
/// The directory, its lock, or its marker could not be written, or it kept vanishing.
pub fn claim_cache(dir: &Path, keyed_to: &Path) -> io::Result<Owned> {
    for _attempt in 0..REMADE {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        lock.lock()?;
        if still_named(&lock, &path)? {
            let marker = Marker {
                schema: SCHEMA,
                pid: std::process::id(),
                started: jiff::Timestamp::now().to_string(),
                kept: false,
                role: "cache",
                keyed_to: keyed_to.to_str().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} is not text a marker can name", keyed_to.display()),
                    )
                })?,
            };
            let mut text = serde_json::to_vec(&marker).map_err(io::Error::other)?;
            text.push(b'\n');
            std::fs::write(dir.join(MARKER), text)?;
            return Ok(Owned { _lock: lock });
        }
    }
    Err(io::Error::other(format!(
        "{} was taken away {REMADE} times while the gate waited to own it",
        dir.display()
    )))
}

/// Marks `dir`, made ahead of Cargo, as a cache.
///
/// # Errors
/// The tag could not be written.
pub fn tag_cache(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(CACHEDIR_TAG), TAG)
}

/// Whether the file `lock` holds is still the one `path` names, which it is not when a collector removed the directory while the lock was awaited.
#[cfg(unix)]
fn still_named(lock: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let held = lock.metadata()?;
    match std::fs::metadata(path) {
        Ok(named) => Ok(named.dev() == held.dev() && named.ino() == held.ino()),
        Err(missing) if missing.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source),
    }
}

/// Whether the file `lock` holds is still the one `path` names; Windows refuses to remove an open file, so it always is.
#[cfg(not(unix))]
fn still_named(_lock: &File, _path: &Path) -> io::Result<bool> {
    Ok(true)
}
