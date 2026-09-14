// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run kept so somebody else can re-derive what it decided.
//!
//! A proof layer removes executions, and a report that says so without the
//! evidence is a claim rather than a proof. These files are the premises: the
//! measurement each target left behind, what its guards recorded about which of
//! its tests reached and infected them, and the catalog with the branch bodies
//! the compiler vouched for. An audit reads them, re-decides every route, and
//! says whether the run's own answers follow — with no access to the engine
//! that produced them.
//!
//! Writing them never fails a run. What could not be written is one file a
//! reader does not have, and a run that ended because it could not write a
//! copy of what it already knows would be a run that failed for nothing.

use std::path::{Path, PathBuf};

use crate::session::Session;

/// The measurement, as a document.
pub const REACHED: &str = "reached-v1.json";

/// What the guards recorded, as a document.
pub const TOUCHED: &str = "touched-v1.json";

/// The catalog, as a document.
pub const CATALOG: &str = "catalog-v1.json";

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
    if let Ok(text) = serde_json::to_string(session.touched()) {
        written.extend(keep(directory, TOUCHED, text.as_bytes()));
    }
    if let Ok(text) = serde_json::to_string(&super::catalog::document(session, options)) {
        written.extend(keep(directory, CATALOG, text.as_bytes()));
    }
    written
}

/// Writes one file and says what it holds, or nothing when it could not be written.
fn keep(directory: &Path, name: &str, bytes: &[u8]) -> Option<Written> {
    let path: PathBuf = directory.join(name);
    crate::replace::file(&path, bytes).ok()?;
    Some(Written {
        file: name.to_owned(),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        digest: crate::id::digest(bytes),
    })
}
