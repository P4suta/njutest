// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the tree contributes to a run's identity: every file under verification, the fuzz corpora apart from them, and the dependencies the lock file resolved.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use rust_mutants::glob::Pattern;
use sha2::{Digest as _, Sha256};

use super::digest::Fields;
use crate::error::{self, ErrorCode};

/// The domain of the tree's own digest.
pub const TREE_DOMAIN: &str = "mjutest-evidence-tree-v1";

/// The domain of the corpus digest.
pub const CORPUS_DOMAIN: &str = "mjutest-evidence-corpus-v1";

/// The domain of the dependency digest.
pub const DEPENDENCIES_DOMAIN: &str = "mjutest-evidence-dependencies-v1";

/// Directories a run writes rather than reads. Nothing under them is part of what the tests are about.
pub const EXCLUDED_DIRECTORIES: [&str; 7] = [
    ".git",
    ".mjutest",
    "reports",
    "dist",
    "target",
    "fuzz/target",
    "fuzz/artifacts",
];

/// Where fuzz corpora live. They are digested apart from the tree because a corpus grows without the code changing, and a run that only grew its corpus is a different run without being a different program.
pub const CORPUS_DIRECTORY: &str = "fuzz/corpus";

/// What one walk of the source tree established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    /// The digest of every file under verification.
    pub tree: String,
    /// The digest of the fuzz corpora.
    pub corpus: String,
    /// How many files entered the tree digest.
    pub files: u32,
    /// How many bytes they held.
    pub bytes: u64,
    /// Every file that entered the tree digest, with its own, so a key over part of the tree can be folded without reading it again.
    pub entries: BTreeMap<String, String>,
}

impl Scan {
    /// The digest of everything under `prefix`, as a slash-separated workspace-relative directory. A directory with nothing under it folds to a digest of its own, which is not the digest of a directory with something under it.
    #[must_use]
    pub fn under(&self, prefix: &str) -> String {
        self.under_matching(prefix, |_path| true)
    }

    /// The digest of the files under `prefix` that `wanted` accepts.
    #[must_use]
    pub fn under_matching(&self, prefix: &str, wanted: impl Fn(&str) -> bool) -> String {
        fold(TREE_DOMAIN, &self.paths_under(prefix, wanted))
    }

    /// The files under `prefix` that `wanted` accepts, with their digests.
    #[must_use]
    pub fn paths_under(
        &self,
        prefix: &str,
        wanted: impl Fn(&str) -> bool,
    ) -> BTreeMap<String, String> {
        let inside = format!("{}/", prefix.trim_end_matches('/'));
        self.entries
            .iter()
            .filter(|(path, _)| prefix.is_empty() || path.starts_with(&inside))
            .filter(|(path, _)| wanted(path))
            .map(|(path, value)| (path.clone(), value.clone()))
            .collect()
    }
}

/// Why the tree could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    /// A directory could not be listed, or a file could not be read.
    #[error("{}: reading {}: {source}", error::EVIDENCE_UNREADABLE.code, path.display())]
    Unreadable {
        /// What could not be read.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A lock file is not the document cargo writes.
    #[error("{}: {path}: {message}", error::EVIDENCE_UNREADABLE.code)]
    Malformed {
        /// What was being read.
        path: String,
        /// What is wrong with it.
        message: String,
    },
}

impl ScanError {
    /// The stable code of every failure of this module: a tree that could not be read is one failure, however it could not be read.
    pub const CODE: ErrorCode = error::EVIDENCE_UNREADABLE;

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unreadable { .. } | Self::Malformed { .. } => Self::CODE,
        }
    }
}

/// What one entry of a walk is, for a visitor that does not care how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Entry {
    /// A symbolic link, and where it points. Its target is what enters a digest, never what it points at.
    Link(String),
    /// A file under verification.
    File(PathBuf),
    /// Something that is neither, which is recorded as being there and not read.
    Irregular,
}

/// Every path under `root` that is part of what a run verifies, in no particular order.
///
/// The rule for what counts lives here and nowhere else, so a reader that
/// digests the tree and a reader that only asks when it last changed agree
/// about which files they are talking about. Two walks with two rules would
/// eventually disagree, and then a watch would sit still through an edit to a
/// file the digest does count.
///
/// # Errors
/// See [`ScanError`].
pub fn walk<V>(
    root: &Path,
    exclude: &[Pattern],
    elsewhere: &[&Path],
    mut visit: V,
) -> Result<(), ScanError>
where
    V: FnMut(&str, Entry) -> Result<(), ScanError>,
{
    let written: Vec<String> = elsewhere
        .iter()
        .filter_map(|path| relative_to(root, path))
        .collect();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|source| ScanError::Unreadable {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ScanError::Unreadable {
                path: directory.clone(),
                source,
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            let kind = entry.file_type().map_err(|source| ScanError::Unreadable {
                path: path.clone(),
                source,
            })?;
            if kind.is_symlink() {
                let target = std::fs::read_link(&path).map_err(|source| ScanError::Unreadable {
                    path: path.clone(),
                    source,
                })?;
                visit(
                    &relative,
                    Entry::Link(target.to_string_lossy().into_owned()),
                )?;
                continue;
            }
            if kind.is_dir() {
                if is_excluded_directory(&relative) || written.contains(&relative) {
                    continue;
                }
                pending.push((path, relative));
                continue;
            }
            if !kind.is_file() {
                visit(&relative, Entry::Irregular)?;
                continue;
            }
            if exclude.iter().any(|pattern| pattern.matches(&relative)) {
                continue;
            }
            visit(&relative, Entry::File(path))?;
        }
    }
    Ok(())
}

/// Reads `root` and digests it: the files under verification into [`Scan::tree`], the fuzz corpora into [`Scan::corpus`].
///
/// `exclude` are the configuration's own patterns. `elsewhere` are directories a run writes rather than reads that are not in [`EXCLUDED_DIRECTORIES`] — cargo's build directory, the user's cache directory — named absolutely or relative to `root`. Neither counts.
///
/// # Errors
/// See [`ScanError`].
pub fn scan(root: &Path, exclude: &[Pattern], elsewhere: &[&Path]) -> Result<Scan, ScanError> {
    let mut tree: BTreeMap<String, String> = BTreeMap::new();
    let mut corpus: BTreeMap<String, String> = BTreeMap::new();
    let mut files = 0u32;
    let mut bytes = 0u64;
    walk(root, exclude, elsewhere, |relative, entry| {
        match entry {
            Entry::Link(target) => {
                let _replaced = tree.insert(relative.to_owned(), format!("link:{target}"));
            }
            Entry::Irregular => {
                let _replaced = tree.insert(relative.to_owned(), "irregular".to_owned());
            }
            Entry::File(path) => {
                let content = std::fs::read(&path).map_err(|source| ScanError::Unreadable {
                    path: path.clone(),
                    source,
                })?;
                let value = hex::encode(Sha256::digest(&content));
                if relative.starts_with(CORPUS_DIRECTORY) {
                    let _replaced = corpus.insert(relative.to_owned(), value);
                    return Ok(());
                }
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
                let _replaced = tree.insert(relative.to_owned(), value);
            }
        }
        Ok(())
    })?;
    Ok(Scan {
        tree: fold(TREE_DOMAIN, &tree),
        corpus: fold(CORPUS_DOMAIN, &corpus),
        files,
        bytes,
        entries: tree,
    })
}

/// The dependencies a lock file resolved, as one digest: every package's name, version, source, and checksum, in a fixed order.
///
/// # Errors
/// Returns [`ScanError::Malformed`] when the text is not the document cargo writes.
pub fn dependencies(lock: &str) -> Result<String, ScanError> {
    let document: LockFile = toml::from_str(lock).map_err(|error| ScanError::Malformed {
        path: "Cargo.lock".to_owned(),
        message: error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned(),
    })?;
    let mut resolved: BTreeMap<String, String> = BTreeMap::new();
    for package in document.package {
        resolved.insert(
            format!("{}@{}", package.name, package.version),
            format!(
                "{}#{}",
                package.source.unwrap_or_default(),
                package.checksum.unwrap_or_default()
            ),
        );
    }
    Ok(fold(DEPENDENCIES_DOMAIN, &resolved))
}

/// Reads `Cargo.lock` beside `root`, or the empty resolution when there is none.
///
/// # Errors
/// See [`ScanError`].
pub fn dependencies_of(root: &Path) -> Result<String, ScanError> {
    let path = root.join("Cargo.lock");
    match std::fs::read_to_string(&path) {
        Ok(text) => dependencies(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(fold(DEPENDENCIES_DOMAIN, &BTreeMap::new()))
        }
        Err(source) => Err(ScanError::Unreadable { path, source }),
    }
}

#[derive(serde::Deserialize)]
struct LockFile {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(serde::Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn fold(domain: &str, entries: &BTreeMap<String, String>) -> String {
    let mut fields = Fields::new(domain);
    fields.list(
        "entries",
        entries
            .iter()
            .map(|(name, value)| format!("{name}\u{0}{value}")),
    );
    fields.finish()
}

fn is_excluded_directory(relative: &str) -> bool {
    EXCLUDED_DIRECTORIES.contains(&relative)
}

/// `path` as a slash-separated path relative to `root`, whether it was given absolute or relative, or `None` when it is not under `root` at all.
fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = if path.is_absolute() {
        let root = root
            .canonicalize()
            .unwrap_or_else(|_error| root.to_path_buf());
        let path = path
            .canonicalize()
            .unwrap_or_else(|_error| path.to_path_buf());
        path.strip_prefix(&root).ok()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
