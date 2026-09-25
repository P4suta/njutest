// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run kept so somebody else can re-derive what it decided.

use std::path::{Path, PathBuf};

use crate::session::Session;

/// The measurement, as a document.
pub const REACHED: &str = "reached-v1.json";

/// What the guards recorded, as a document.
pub const TOUCHED: &str = "touched-v1.json";

/// The catalog, as a document.
pub const CATALOG: &str = "catalog-v1.json";

/// Every carried record the run believed, as a document.
pub const CARRIED: &str = crate::carry::FILE;

/// Which bodies are sealed and each unit's skeleton, as a document.
pub const SKELETONS: &str = crate::skeleton::FILE;

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

/// Why the retained premises of a report could not be written completely.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EvidenceError {
    /// The evidence directory could not be created.
    #[error("creating evidence directory {}: {source}", path.display())]
    Create {
        /// The directory that was needed.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// One retained document could not be represented as JSON.
    #[error("serializing retained evidence {file}: {source}")]
    Serialize {
        /// The document that could not be represented.
        file: &'static str,
        /// The serializer's reason.
        #[source]
        source: serde_json::Error,
    },
    /// One retained document could not be committed to disk.
    #[error("writing retained evidence {}: {source}", path.display())]
    Write {
        /// The document that could not be written.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// One retained document is larger than the wire format can represent.
    #[error("retained evidence {file} has {bytes} bytes, which does not fit its u64 size field")]
    SizeOverflow {
        /// The document whose exact size did not fit.
        file: &'static str,
        /// The exact in-memory size.
        bytes: usize,
    },
    /// The carried records the run believed could not be read.
    #[error("reading the carried records this run believed: {source}")]
    Carried {
        /// The engine's reason.
        #[source]
        source: crate::EngineError,
    },
    /// The catalog could not be represented without changing workspace or mutation bytes.
    #[error("constructing retained catalog evidence: {source}")]
    Catalog {
        /// The exact catalog construction failure.
        #[source]
        source: crate::EngineError,
    },
}

/// Writes everything an audit re-derives a run's proofs from, and reports what it wrote.
///
/// # Errors
/// Returns the first directory, serialization, exact-size, or durable-write failure.
/// No document is claimed unless its complete bytes were committed.
pub fn write(
    session: &Session,
    directory: &Path,
    options: &crate::session::PrepareOptions,
) -> Result<Vec<Written>, EvidenceError> {
    let mut written = Vec::new();
    std::fs::create_dir_all(directory).map_err(|source| EvidenceError::Create {
        path: directory.to_path_buf(),
        source,
    })?;
    let reached =
        serde_json::to_vec(session.reached()).map_err(|source| EvidenceError::Serialize {
            file: REACHED,
            source,
        })?;
    written.push(keep(directory, REACHED, &reached)?);
    let touched =
        serde_json::to_vec(session.touched()).map_err(|source| EvidenceError::Serialize {
            file: TOUCHED,
            source,
        })?;
    written.push(keep(directory, TOUCHED, &touched)?);
    let skeletons =
        serde_json::to_vec(&session.skeletons()).map_err(|source| EvidenceError::Serialize {
            file: SKELETONS,
            source,
        })?;
    written.push(keep(directory, SKELETONS, &skeletons)?);
    let believed = session
        .carried_evidence()
        .map_err(|source| EvidenceError::Carried { source })?;
    let carried = serde_json::to_vec(&believed).map_err(|source| EvidenceError::Serialize {
        file: CARRIED,
        source,
    })?;
    written.push(keep(directory, CARRIED, &carried)?);
    let catalog_document = super::catalog::document(session, options)
        .map_err(|source| EvidenceError::Catalog { source })?;
    let catalog =
        serde_json::to_vec(&catalog_document).map_err(|source| EvidenceError::Serialize {
            file: CATALOG,
            source,
        })?;
    written.push(keep(directory, CATALOG, &catalog)?);
    Ok(written)
}

/// Writes one file and says what it holds, or nothing when it could not be written.
fn keep(directory: &Path, name: &'static str, bytes: &[u8]) -> Result<Written, EvidenceError> {
    let exact_bytes =
        u64::try_from(bytes.len()).map_err(|_overflow| EvidenceError::SizeOverflow {
            file: name,
            bytes: bytes.len(),
        })?;
    let path: PathBuf = directory.join(name);
    crate::replace::file(&path, bytes).map_err(|failure| EvidenceError::Write {
        path: failure.path,
        source: failure.source,
    })?;
    Ok(Written {
        file: name.to_owned(),
        bytes: exact_bytes,
        digest: crate::id::digest(bytes),
    })
}
