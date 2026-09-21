// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of what runs left behind on purpose.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// The name of the shape.
pub const SCHEMA: &str = "njutest-kept-temp-v1";

/// Where the ledger lives, relative to the workspace root.
const FILE_NAME: &str = ".njutest/kept-temp-v1.json";

/// Where the ledger of kept directories lives under `root`.
///
/// The path is composed here rather than exported as a constant others join:
/// a constant spelling a structure is a layout every holder decides for
/// itself, including the tests, and a layout the tests have decided is one no
/// configuration can move.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE_NAME)
}

/// What runs have left behind on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    /// [`SCHEMA`].
    pub schema: String,
    /// Every directory a run preserved, newest last.
    pub kept: Vec<Kept>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            kept: Vec::new(),
        }
    }
}

/// One directory one run preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    /// The run that preserved it.
    pub run_id: String,
    /// When it did.
    pub at: String,
    /// The directory, absolutely.
    pub path: String,
}

/// The ledger at `root`, or an empty one when no ledger exists yet.
///
/// # Errors
/// A present ledger is unreadable, malformed, or belongs to another schema.
pub fn read(root: &Path) -> std::io::Result<Ledger> {
    let path = path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Ledger::default());
        }
        Err(error) => return Err(error),
    };
    let ledger: Ledger = crate::strictjson::decode_str(&text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if ledger.schema != SCHEMA {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is not a {SCHEMA} ledger", path.display()),
        ));
    }
    Ok(ledger)
}

/// Records that `run_id` preserved `paths`, and returns where the ledger went.
///
/// # Errors
/// The ledger could not be written, which never fails a run: what it is for is
/// telling a person what is on their disk.
pub fn record(
    root: &Path,
    run_id: &str,
    at: Timestamp,
    paths: &[PathBuf],
) -> std::io::Result<PathBuf> {
    let mut ledger = read(root)?;
    for path in paths {
        let entry = Kept {
            run_id: run_id.to_owned(),
            at: at.to_string(),
            path: path.display().to_string(),
        };
        ledger.kept.retain(|kept| kept.path != entry.path);
        ledger.kept.push(entry);
    }
    let path = path(root);
    let text = serde_json::to_string_pretty(&ledger).map_err(std::io::Error::other)?;
    rust_mutants::replace::file(&path, format!("{text}\n").as_bytes())
        .map_err(|failure| failure.source)?;
    Ok(path)
}

/// Removes from the ledger every directory that is no longer there, and returns what is left. A directory that went away without this program's help is not something to keep telling a person about.
#[must_use]
pub fn forget_gone(ledger: &Ledger) -> Ledger {
    Ledger {
        schema: ledger.schema.clone(),
        kept: ledger
            .kept
            .iter()
            .filter(
                |kept| match std::fs::symlink_metadata(Path::new(&kept.path)) {
                    Ok(metadata) => metadata.file_type().is_dir(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(_unreadable) => true,
                },
            )
            .cloned()
            .collect(),
    }
}
