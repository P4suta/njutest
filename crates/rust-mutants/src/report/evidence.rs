// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run kept so somebody else can re-derive what it decided.
//!
//! A proof layer removes executions, and a report that says so without the
//! evidence is a claim rather than a proof. These files are the premises: the
//! measurement each target left behind, the catalog with the branch bodies the
//! compiler vouched for, and the log each probe process appended to. An audit
//! reads them, re-decides every route, and says whether the run's own answers
//! follow — with no access to the engine that produced them.
//!
//! Writing them never fails a run. What could not be written is one file a
//! reader does not have, and a run that ended because it could not write a
//! copy of what it already knows would be a run that failed for nothing.

use std::path::{Path, PathBuf};

use crate::session::Session;

/// The measurement, as a document.
pub const REACHED: &str = "reached-v1.json";

/// The catalog, as a document.
pub const CATALOG: &str = "catalog-v1.json";

/// The directory the probe logs are copied into.
pub const PROBE: &str = "probe";

/// One file a run kept, with what a reader can check it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The path, relative to the directory it was written into.
    pub file: String,
    /// How many bytes it holds.
    pub bytes: u64,
    /// The lowercase hex SHA-256 of those bytes.
    pub digest: String,
}

/// Writes everything an audit re-derives a run's proofs from, and reports what it wrote.
///
/// Nothing here is an error a run ends on: a file that could not be written
/// is one an audit will call unaudited, which is the honest answer, and is
/// better than a run that failed because it could not copy what it knows.
pub fn write(
    session: &Session,
    directory: &Path,
    options: &crate::session::PrepareOptions,
) -> Vec<Written> {
    let mut written = Vec::new();
    if std::fs::create_dir_all(directory).is_err() {
        return written;
    }
    if let Ok(text) = serde_json::to_string(session.reached()) {
        written.extend(keep(directory, REACHED, text.as_bytes()));
    }
    if let Ok(text) = serde_json::to_string(&super::catalog::document(session, options)) {
        written.extend(keep(directory, CATALOG, text.as_bytes()));
    }
    written.extend(probe_logs(session, directory));
    written
}

/// Copies every probe log the pass left behind.
fn probe_logs(session: &Session, directory: &Path) -> Vec<Written> {
    let from = crate::probe::tree::logs_dir(session.target_dir());
    let into = directory.join(PROBE);
    let Ok(entries) = std::fs::read_dir(&from) else {
        return Vec::new();
    };
    if std::fs::create_dir_all(&into).is_err() {
        return Vec::new();
    }
    let mut written = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        written.extend(keep(&into, &name, &bytes).into_iter().map(|one| Written {
            file: format!("{PROBE}/{}", one.file),
            ..one
        }));
    }
    written
}

/// Writes one file and says what it holds, or nothing when it could not be written.
fn keep(directory: &Path, name: &str, bytes: &[u8]) -> Option<Written> {
    let path: PathBuf = directory.join(name);
    std::fs::write(&path, bytes).ok()?;
    Some(Written {
        file: name.to_owned(),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        digest: crate::id::digest(bytes),
    })
}
