// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a measurement a selection reads is kept: one document, and the measured bytes of every source file by digest.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rust_mutants::id::HexDigest;
use rust_mutants::select::Measurement;

use crate::error::ErrorCode;

/// The directory under the report directory a measurement is kept in.
pub const DIRECTORY: &str = "reach";

/// The document's file name.
pub const DOCUMENT_NAME: &str = "njutest-reach-v1.json";

/// The directory the measured bytes are kept in, each file named by its SHA-256.
const BLOBS: &str = "blobs";

/// The one schema a measurement document is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Schema {
    /// The first.
    #[serde(rename = "njutest-reach-v1")]
    V1,
}

/// What `njutest measure` writes and `njutest select` reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Which document this is.
    pub schema: Schema,
    /// What the measured tree establishes.
    pub measurement: Measurement,
}

/// Why a measurement could not be kept or read back.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReachError {
    /// The document or a measured file could not be written.
    #[error("{}: cannot write {path}: {source}", crate::error::MEASUREMENT_UNWRITABLE.code)]
    Unwritable {
        /// Where.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// There is no measurement, or it is not one this release reads.
    #[error("{}: {path}: {message}", crate::error::MEASUREMENT_UNREADABLE.code)]
    Unreadable {
        /// Where it was looked for.
        path: PathBuf,
        /// Why it could not be read.
        message: String,
    },
}

impl ReachError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unwritable { .. } => crate::error::MEASUREMENT_UNWRITABLE,
            Self::Unreadable { .. } => crate::error::MEASUREMENT_UNREADABLE,
        }
    }
}

/// Where the measurement of the tree whose report directory is `reports` lives.
#[must_use]
pub fn directory(reports: &Path) -> PathBuf {
    reports.join(DIRECTORY)
}

/// Writes `document` and every measured file of `sources` under `directory`, replacing any measurement kept there before and every measured file it no longer names.
///
/// # Errors
/// [`ReachError::Unwritable`] naming the first file that could not be written.
pub fn keep(
    directory: &Path,
    document: &Document,
    sources: &BTreeMap<HexDigest, Vec<u8>>,
) -> Result<(), ReachError> {
    let unwritable = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ReachError::Unwritable { path, source }
    };
    let blobs = directory.join(BLOBS);
    std::fs::create_dir_all(&blobs).map_err(unwritable(&blobs))?;
    for (digest, bytes) in sources {
        let path = blobs.join(digest.as_str());
        if measured(directory, digest).is_some() {
            continue;
        }
        replaced(&path, bytes).map_err(unwritable(&path))?;
    }
    let text = serde_json::to_vec_pretty(document).map_err(|source| ReachError::Unwritable {
        path: directory.join(DOCUMENT_NAME),
        source: std::io::Error::other(source),
    })?;
    let path = directory.join(DOCUMENT_NAME);
    replaced(&path, &text).map_err(unwritable(&path))?;
    let listing = std::fs::read_dir(&blobs).map_err(unwritable(&blobs))?;
    for entry in listing {
        let entry = entry.map_err(unwritable(&blobs))?;
        let named = entry.file_name();
        let kept = match named.to_str().map(HexDigest::try_from) {
            Some(Ok(digest)) => sources.contains_key(&digest),
            Some(Err(_)) | None => false,
        };
        if !kept {
            let path = entry.path();
            std::fs::remove_file(&path).map_err(unwritable(&path))?;
        }
    }
    Ok(())
}

/// Writes `bytes` to a file beside `path` and renames it over `path`, so a reader sees the old file or the new one and never a part.
fn replaced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("partial");
    let mut file = std::fs::File::create(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&partial, path)
}

/// The measurement kept under `directory`.
///
/// # Errors
/// [`ReachError::Unreadable`] when there is none, or it is not a document this release reads.
pub fn read(directory: &Path) -> Result<Document, ReachError> {
    let path = directory.join(DOCUMENT_NAME);
    let text = std::fs::read_to_string(&path).map_err(|source| ReachError::Unreadable {
        path: path.clone(),
        message: format!("{source}; `njutest measure` writes one"),
    })?;
    crate::strictjson::decode_str(&text).map_err(|source| ReachError::Unreadable {
        path,
        message: source.to_string(),
    })
}

/// The measured bytes of the file whose SHA-256 is `digest`, or nothing when they were not kept or are not those bytes.
#[must_use]
pub fn measured(directory: &Path, digest: &HexDigest) -> Option<String> {
    let path = directory.join(BLOBS).join(digest.as_str());
    let Ok(bytes) = std::fs::read(path) else {
        return None;
    };
    if HexDigest::of(&bytes) != *digest {
        return None;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return None;
    };
    Some(text)
}
