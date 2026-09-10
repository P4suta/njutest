// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of what runs left behind on purpose.
//!
//! A keep is recorded where it outlives the run, so a successful untraced run
//! still accounts for what it preserved (ADR 0006, decision 7). The ledger
//! names a directory; the directory's own marker says whether it may be
//! removed, because a path in an editable file may not authorize a recursive
//! delete (decision 8).

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// The name of the shape.
pub const SCHEMA: &str = "mjutest-kept-temp-v1";

/// Where the ledger lives, relative to the workspace root.
pub const FILE_NAME: &str = ".mjutest/kept-temp-v1.json";

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

/// The ledger at `root`, or an empty one when there is none or it cannot be read. A ledger this release cannot read is replaced rather than obeyed: it authorizes nothing on its own.
#[must_use]
pub fn read(root: &Path) -> Ledger {
    std::fs::read_to_string(root.join(FILE_NAME))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
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
    let mut ledger = read(root);
    for path in paths {
        let entry = Kept {
            run_id: run_id.to_owned(),
            at: at.to_string(),
            path: path.display().to_string(),
        };
        ledger.kept.retain(|kept| kept.path != entry.path);
        ledger.kept.push(entry);
    }
    let path = root.join(FILE_NAME);
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
            .filter(|kept| Path::new(&kept.path).is_dir())
            .cloned()
            .collect(),
    }
}
