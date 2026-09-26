// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a target directory was built from, so that no unit cargo judges fresh by a file's time was compiled from bytes the tree no longer holds.

use std::collections::BTreeMap;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{CargoError, CargoErrorKind};

/// The schema of the record a target directory keeps of what its members were last built from.
pub const LEDGER_SCHEMA: &str = "rust-mutants-built-v1";

/// The record's file name inside the target directory it describes.
pub const LEDGER_NAME: &str = "rust-mutants-built-v1.json";

/// The directory cargo keeps one fingerprint per unit in, inside each profile directory.
const FINGERPRINTS: &str = ".fingerprint";

/// How many hex digits cargo spells a unit's metadata hash with, after the package name and a hyphen.
const METADATA_HEX_LENGTH: usize = 16;

/// The copy buffer.
const READ_BUFFER: usize = 64 * 1024;

/// One file of a member, by its path in the tree and where the copy holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFile {
    /// The path relative to the tree's root, with forward slashes, which is the same wherever the copy sits.
    pub rel_path: String,
    /// Where the copy holds it.
    pub path: PathBuf,
}

/// A workspace member a target directory may hold units of, named as cargo names its fingerprints, with every file of the tree under its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The package name, which begins the name of every fingerprint cargo keeps for it.
    pub name: String,
    /// Every file of the tree under the member's directory, in path order.
    pub files: Vec<MemberFile>,
}

/// A directory cargo builds into, with the members whose units it may hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildDir {
    path: PathBuf,
    members: Vec<Member>,
}

/// The record a target directory keeps: every member's files as the last build that could write its fingerprints found them.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema: String,
    members: BTreeMap<String, Settled>,
}

/// What one member's files held when its fingerprints were last removed, and when that was.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settled {
    digest: String,
    since: jiff::Timestamp,
}

impl BuildDir {
    /// A target directory and the members a build into it may compile.
    #[must_use]
    pub const fn new(path: PathBuf, members: Vec<Member>) -> Self {
        Self { path, members }
    }

    /// The directory cargo is told to build into.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A directory inside this one that cargo builds the same members into on its own terms, with its own record.
    #[must_use]
    pub fn nested(&self, name: &str) -> Self {
        Self {
            path: self.path.join(name),
            members: self.members.clone(),
        }
    }

    /// Makes cargo compile again every unit of a member whose files differ from what this directory last built it from, and dates every member's files to when their bytes last moved.
    ///
    /// # Errors
    /// [`CargoErrorKind::BuildLedger`] when the record cannot be read, is not one this release writes, or cannot be written, when a member's file cannot be read or dated, or when a fingerprint cannot be removed.
    pub fn settle(&self) -> Result<(), CargoError> {
        let ledger_path = self.path.join(LEDGER_NAME);
        let mut ledger = read_ledger(&ledger_path)?;
        let mut moved = Vec::new();
        for member in &self.members {
            let digest = member_digest(member)?;
            let unchanged = ledger
                .members
                .get(&member.name)
                .is_some_and(|settled| settled.digest == digest);
            if !unchanged {
                moved.push((member.name.as_str(), digest));
            }
        }
        if !moved.is_empty() {
            let names: Vec<&str> = moved.iter().map(|(name, _)| *name).collect();
            invalidate(&self.path, &names)?;
            let since =
                jiff::Timestamp::try_from(std::time::SystemTime::now()).map_err(|error| {
                    CargoError::new(
                        CargoErrorKind::BuildLedger,
                        format!(
                            "the clock cannot date the record for {}: {error}",
                            self.path.display()
                        ),
                    )
                })?;
            for (name, digest) in moved {
                ledger
                    .members
                    .insert(name.to_owned(), Settled { digest, since });
            }
            write_ledger(&self.path, &ledger_path, &ledger)?;
        }
        for member in &self.members {
            let Some(settled) = ledger.members.get(&member.name) else {
                continue;
            };
            let since = std::time::SystemTime::from(settled.since);
            for file in &member.files {
                dated(&file.path, since)?;
            }
        }
        Ok(())
    }
}

fn ledger_error(message: String, source: io::Error) -> CargoError {
    CargoError::new(CargoErrorKind::BuildLedger, message).with_source(source)
}

fn read_ledger(path: &Path) -> Result<Ledger, CargoError> {
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Ledger {
                schema: LEDGER_SCHEMA.to_owned(),
                members: BTreeMap::new(),
            });
        }
        Err(error) => {
            return Err(ledger_error(
                format!("{} could not be read", path.display()),
                error,
            ));
        }
    };
    let ledger: Ledger = crate::strictjson::decode_slice(&text).map_err(|error| {
        CargoError::new(
            CargoErrorKind::BuildLedger,
            format!("{} is not a record this release writes", path.display()),
        )
        .with_source(error)
    })?;
    if ledger.schema != LEDGER_SCHEMA {
        return Err(CargoError::new(
            CargoErrorKind::BuildLedger,
            format!(
                "{} is a {} record, not {LEDGER_SCHEMA}",
                path.display(),
                ledger.schema
            ),
        ));
    }
    Ok(ledger)
}

fn write_ledger(dir: &Path, path: &Path, ledger: &Ledger) -> Result<(), CargoError> {
    std::fs::create_dir_all(dir)
        .map_err(|error| ledger_error(format!("{} could not be created", dir.display()), error))?;
    let text = serde_json::to_vec_pretty(ledger).map_err(|error| {
        CargoError::new(
            CargoErrorKind::BuildLedger,
            format!("the record for {} could not be written", dir.display()),
        )
        .with_source(error)
    })?;
    let staged = dir.join(format!("{LEDGER_NAME}.{}", std::process::id()));
    std::fs::write(&staged, text).map_err(|error| {
        ledger_error(format!("{} could not be written", staged.display()), error)
    })?;
    std::fs::rename(&staged, path).map_err(|error| {
        ledger_error(
            format!("{} could not be put in place", path.display()),
            error,
        )
    })
}

/// The digest of a member's files: each path in the tree beside the digest of its bytes, or beside the word that it is gone.
fn member_digest(member: &Member) -> Result<String, CargoError> {
    let mut hasher = Sha256::new();
    for file in &member.files {
        hasher.update(file.rel_path.as_bytes());
        hasher.update([0]);
        match file_digest(&file.path) {
            Ok(digest) => hasher.update(digest),
            Err(error) if error.kind() == io::ErrorKind::NotFound => hasher.update(b"absent"),
            Err(error) => {
                return Err(ledger_error(
                    format!("{} could not be read", file.path.display()),
                    error,
                ));
            }
        }
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Gives a file of a member the time its member's bytes last moved, which is older than every unit built from them and newer than every unit built from anything else.
fn dated(path: &Path, since: std::time::SystemTime) -> Result<(), CargoError> {
    match for_dating(path).and_then(|file| file.set_modified(since)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ledger_error(
            format!("{} could not be dated", path.display()),
            error,
        )),
    }
}

/// Opens a file only as far as setting its times needs, which a file its owner made read-only still allows.
#[cfg(not(windows))]
fn for_dating(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Opens a file only as far as setting its times needs, which a file its owner made read-only still allows.
#[cfg(windows)]
fn for_dating(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .open(path)
}

fn file_digest(path: &Path) -> io::Result<[u8; 32]> {
    let mut input = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; READ_BUFFER];
    loop {
        let read = input.read(&mut buffer)?;
        let Some(chunk) = buffer.get(..read) else {
            return Err(io::Error::other(
                "a read reported more bytes than its buffer holds",
            ));
        };
        if chunk.is_empty() {
            return Ok(hasher.finalize().into());
        }
        hasher.update(chunk);
    }
}

/// Removes every fingerprint of the moved members from each profile directory of `dir`, with and without `--target`, passing over a directory that keeps its own record.
fn invalidate(dir: &Path, moved: &[&str]) -> Result<(), CargoError> {
    for outer in subdirectories(dir)? {
        if holds_ledger(&outer)? {
            continue;
        }
        invalidate_in(&outer.join(FINGERPRINTS), moved)?;
        for inner in subdirectories(&outer)? {
            invalidate_in(&inner.join(FINGERPRINTS), moved)?;
        }
    }
    Ok(())
}

fn invalidate_in(fingerprints: &Path, moved: &[&str]) -> Result<(), CargoError> {
    for unit in subdirectories(fingerprints)? {
        let Some(name) = unit.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        if moved.iter().any(|member| fingerprint_of(name, member)) {
            crate::tempowner::remove_tree(&unit).map_err(|error| {
                ledger_error(format!("{} could not be removed", unit.display()), error)
            })?;
        }
    }
    Ok(())
}

/// Whether `name` is the fingerprint directory cargo keeps for a unit of `member`: the package name, a hyphen, and the unit's metadata hash.
#[must_use]
pub fn fingerprint_of(name: &str, member: &str) -> bool {
    name.strip_prefix(member)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|hash| {
            hash.len() == METADATA_HEX_LENGTH
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn holds_ledger(dir: &Path) -> Result<bool, CargoError> {
    let path = dir.join(LEDGER_NAME);
    match std::fs::symlink_metadata(&path) {
        Ok(_metadata) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ledger_error(
            format!("{} could not be inspected", path.display()),
            error,
        )),
    }
}

/// Every directory directly inside `dir`, never following a link, and none when `dir` is not there.
fn subdirectories(dir: &Path) -> Result<Vec<PathBuf>, CargoError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.kind() == io::ErrorKind::NotADirectory =>
        {
            return Ok(Vec::new());
        }
        Err(error) => {
            return Err(ledger_error(
                format!("{} could not be listed", dir.display()),
                error,
            ));
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ledger_error(format!("{} could not be listed", dir.display()), error)
        })?;
        let kind = entry.file_type().map_err(|error| {
            ledger_error(
                format!("{} could not be inspected", entry.path().display()),
                error,
            )
        })?;
        if kind.is_dir() {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}
