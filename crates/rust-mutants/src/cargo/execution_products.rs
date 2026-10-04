// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution copies are measured as they stand while their original compiler products stay immutable.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use super::build_cache::{File, Preparation, file, tree};
use super::{BuildDir, CargoError, CargoErrorKind, Message};
use crate::trace::Recorder;

/// Mutable execution products with a retained immutable compiler origin, separate from compiler proof.
#[derive(Debug)]
pub struct ExecutionProducts {
    messages: Vec<Message>,
}

impl ExecutionProducts {
    /// Creates each execution copy once and retains its later observed bytes without repairing them.
    ///
    /// # Errors
    /// The retained origin, execution owner or publication cannot be read or verified.
    pub fn of(
        messages: &[Message],
        target: &BuildDir,
        trace: &Recorder,
    ) -> Result<Self, CargoError> {
        let mut owned = Self {
            messages: messages.to_vec(),
        };
        let roots: BTreeSet<PathBuf> = messages
            .iter()
            .filter_map(|message| {
                if let Message::CompilerArtifact(artifact) = message {
                    artifact
                        .executable
                        .as_ref()
                        .and_then(|path| archive(path, target.path()))
                        .map(Path::to_path_buf)
                } else {
                    None
                }
            })
            .collect();
        if roots.is_empty() {
            return Ok(owned);
        }
        let preparation = Preparation::own(target.path(), trace).map_err(unavailable)?;
        let mut copies = BTreeMap::new();
        for origin in roots {
            let name = origin
                .file_name()
                .ok_or_else(|| unavailable(io::Error::other("execution origin")))?;
            let destination = target
                .path()
                .join("rust-mutants-execution-products")
                .join(name);
            publish(&origin, &destination).map_err(unavailable)?;
            copies.insert(origin, destination);
        }
        for message in &mut owned.messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact
                    .filenames
                    .iter_mut()
                    .chain(&mut artifact.executable)
                {
                    for (origin, destination) in &copies {
                        match path.strip_prefix(origin) {
                            Ok(relative) => {
                                *path = destination.join(relative);
                                break;
                            }
                            Err(_different_origin) => {}
                        }
                    }
                }
            }
        }
        trace.note(
            "execution-products",
            &serde_json::to_string(&copies)
                .map_err(|source| unavailable(io::Error::other(source)))?,
        );
        drop(preparation);
        Ok(owned)
    }

    /// The original Cargo messages with only execution paths placed in their separate mutable owner.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
}

fn unavailable(source: io::Error) -> CargoError {
    CargoError::new(
        CargoErrorKind::BuildLedger,
        "cannot publish owned execution products",
    )
    .with_source(source)
}

fn archive<'a>(path: &'a Path, target: &Path) -> Option<&'a Path> {
    path.ancestors().find(|ancestor| {
        ancestor.parent() == Some(target.join(super::COMPILATIONS).as_path())
            && ancestor
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(super::names_products)
    })
}

fn inventory(root: &Path) -> io::Result<BTreeMap<PathBuf, File>> {
    let mut files = BTreeMap::new();
    tree(root, &PathBuf::new(), &mut files)?;
    files
        .into_iter()
        .map(|(path, state)| {
            Ok((
                path.strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_path_buf(),
                state,
            ))
        })
        .collect()
}

fn publish(origin: &Path, destination: &Path) -> io::Result<()> {
    let original = inventory(origin)?;
    let marker = destination.join("compiler-origin.json");
    match std::fs::read(&marker) {
        Ok(bytes) => {
            let held: BTreeMap<PathBuf, File> =
                crate::strictjson::decode_slice(&bytes).map_err(io::Error::other)?;
            if held != original {
                return Err(io::Error::other("the immutable execution origin changed"));
            }
            return Ok(());
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(source),
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("execution copy owner"))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix("execution-")
        .tempdir_in(parent)?;
    for (relative, state) in &original {
        let source = origin.join(relative);
        let copied = staging.path().join(relative);
        let parent = copied
            .parent()
            .ok_or_else(|| io::Error::other("execution file owner"))?;
        std::fs::create_dir_all(parent)?;
        std::fs::copy(&source, &copied)?;
        if file(&source)? != *state || file(&copied)? != *state {
            return Err(io::Error::other(
                "the execution source changed during publication",
            ));
        }
    }
    std::fs::write(
        staging.path().join("compiler-origin.json"),
        serde_json::to_vec(&original).map_err(io::Error::other)?,
    )?;
    std::fs::rename(staging.path(), destination)
}

#[cfg(test)]
mod tests;
