// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the tree contributes to a run's identity: every file under verification, the fuzz corpora apart from them, and the dependencies the lock file resolved.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use rust_mutants::glob::Pattern;
use sha2::{Digest as _, Sha256};

use super::digest::Fields;
use crate::error::{self, ErrorCode};

/// The domain of the tree's own digest.
pub const TREE_DOMAIN: &str = "njutest-evidence-tree-v1";

/// The domain of the corpus digest.
pub const CORPUS_DOMAIN: &str = "njutest-evidence-corpus-v1";

/// The domain of the dependency digest.
pub const DEPENDENCIES_DOMAIN: &str = "njutest-evidence-dependencies-v1";

/// Directories every project writes rather than reads. Nothing under them is part of what the tests are about.
pub const EXCLUDED_DIRECTORIES: [&str; 6] = [
    ".git",
    ".njutest",
    "dist",
    "target",
    "fuzz/target",
    "fuzz/artifacts",
];

/// What a walk of one project leaves out: what any run writes, and where this one was told to keep its reports.
///
/// The report directory used to be a word in the list above, so a project
/// that moved it digested its own reports into the run identity, watched
/// itself write them, and asked git about them. Where a project writes is
/// configuration, so the list is a value rather than a constant.
#[derive(Debug, Clone)]
pub struct Excluded(Vec<String>);

impl Excluded {
    /// The directories a project that keeps its reports at `reports` does not verify.
    ///
    /// # Errors
    /// Refuses a report directory whose exact platform spelling cannot enter
    /// the UTF-8 evidence and git-exclusion protocols.
    pub fn beside(reports: &Path) -> Result<Self, ScanError> {
        let report = exact_path(reports)?;
        let mut names: Vec<String> = EXCLUDED_DIRECTORIES
            .iter()
            .map(|name| (*name).to_owned())
            .chain(std::iter::once(report))
            .filter(|name| !name.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(Self(names))
    }

    /// Every one of them, as a git command wants them.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.0.iter().map(String::as_str).collect()
    }

    /// Whether `relative` names one.
    #[must_use]
    pub fn holds(&self, relative: &str) -> bool {
        self.0.iter().any(|name| name == relative)
    }
}

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
    /// The exact tree census did not fit the durable evidence counters.
    #[error(
        "{}: the verified tree's {field} exceed the evidence format",
        error::EVIDENCE_UNREADABLE.code
    )]
    CensusOverflow {
        /// Which exact counter could not represent the tree.
        field: &'static str,
    },
    /// Two walk entries normalized to the same evidence identity.
    #[error(
        "{}: more than one tree entry normalizes to {path:?}",
        error::EVIDENCE_UNREADABLE.code
    )]
    DuplicatePath {
        /// The ambiguous normalized path.
        path: String,
    },
    /// A platform path cannot be represented byte-for-byte by the UTF-8
    /// evidence protocol.
    #[error(
        "{}: path {} is not valid UTF-8",
        error::EVIDENCE_UNREADABLE.code,
        path.display()
    )]
    PathNotUtf8 {
        /// The exact platform path that was refused.
        path: PathBuf,
    },
}

impl ScanError {
    /// The stable code of every failure of this module: a tree that could not be read is one failure, however it could not be read.
    pub const CODE: ErrorCode = error::EVIDENCE_UNREADABLE;

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unreadable { .. }
            | Self::Malformed { .. }
            | Self::CensusOverflow { .. }
            | Self::DuplicatePath { .. }
            | Self::PathNotUtf8 { .. } => Self::CODE,
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

/// What one walk of a tree leaves out: what the configuration excluded, what is kept elsewhere, and what any run writes.
#[derive(Debug, Clone, Copy)]
pub struct Bounds<'a> {
    /// Patterns the configuration excluded, matched against each `/`-normalized relative path.
    pub exclude: &'a [Pattern],
    /// Directories kept outside the tree, named either relatively or absolutely.
    pub elsewhere: &'a [&'a Path],
    /// Directories a run writes rather than reads.
    pub excluded: &'a Excluded,
}

/// Every path under `root` that is part of what a run verifies, in no particular order.
///
/// # Errors
/// See [`ScanError`].
pub fn walk<V>(root: &Path, within: &Bounds<'_>, mut visit: V) -> Result<(), ScanError>
where
    V: FnMut(&str, Entry) -> Result<(), ScanError>,
{
    let Bounds {
        exclude,
        elsewhere,
        excluded,
    } = *within;
    let mut written = Vec::new();
    for path in elsewhere {
        push_relative_to(root, path, &mut written)?;
    }
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
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| ScanError::PathNotUtf8 {
                    path: directory.join(name),
                })?;
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
                let target = exact_path(&target)?;
                visit(&relative, Entry::Link(target))?;
                continue;
            }
            if kind.is_dir() {
                if excluded.holds(&relative) || written.contains(&relative) {
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
/// # Errors
/// See [`ScanError`].
pub fn scan(root: &Path, within: &Bounds<'_>) -> Result<Scan, ScanError> {
    let mut tree: BTreeMap<String, String> = BTreeMap::new();
    let mut corpus: BTreeMap<String, String> = BTreeMap::new();
    let mut files = 0u32;
    let mut bytes = 0u64;
    walk(root, within, |relative, entry| {
        match entry {
            Entry::Link(target) => {
                if tree
                    .insert(relative.to_owned(), format!("link:{target}"))
                    .is_some()
                {
                    return Err(ScanError::DuplicatePath {
                        path: relative.to_owned(),
                    });
                }
            }
            Entry::Irregular => {
                if tree
                    .insert(relative.to_owned(), "irregular".to_owned())
                    .is_some()
                {
                    return Err(ScanError::DuplicatePath {
                        path: relative.to_owned(),
                    });
                }
            }
            Entry::File(path) => {
                let content = std::fs::read(&path).map_err(|source| ScanError::Unreadable {
                    path: path.clone(),
                    source,
                })?;
                let value = hex::encode(Sha256::digest(&content));
                if relative.starts_with(CORPUS_DIRECTORY) {
                    if corpus.insert(relative.to_owned(), value).is_some() {
                        return Err(ScanError::DuplicatePath {
                            path: relative.to_owned(),
                        });
                    }
                    return Ok(());
                }
                files = files
                    .checked_add(1)
                    .ok_or(ScanError::CensusOverflow { field: "files" })?;
                let file_bytes = u64::try_from(content.len())
                    .map_err(|_too_large| ScanError::CensusOverflow { field: "bytes" })?;
                bytes = bytes
                    .checked_add(file_bytes)
                    .ok_or(ScanError::CensusOverflow { field: "bytes" })?;
                if tree.insert(relative.to_owned(), value).is_some() {
                    return Err(ScanError::DuplicatePath {
                        path: relative.to_owned(),
                    });
                }
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
    let LockFile { version, package } = document;
    let mut resolved: BTreeMap<String, String> =
        BTreeMap::from([("@lock-version".to_owned(), version.to_string())]);
    for package in package.unwrap_or_default() {
        let LockPackage {
            name,
            version,
            source,
            checksum,
            dependencies: _dependencies,
        } = package;
        resolved.insert(
            format!("{name}@{version}"),
            format!(
                "{}#{}",
                source.unwrap_or_default(),
                checksum.unwrap_or_default()
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
#[serde(deny_unknown_fields)]
struct LockFile {
    version: u32,
    package: Option<Vec<LockPackage>>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    dependencies: Option<Vec<String>>,
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

/// `path` as a slash-separated path relative to `root`, whether it was given absolute or relative, or `None` when it is not under `root` at all.
fn push_relative_to(
    root: &Path,
    path: &Path,
    relative_paths: &mut Vec<String>,
) -> Result<(), ScanError> {
    let relative = if path.is_absolute() {
        let root = match root.canonicalize() {
            Ok(root) => root,
            Err(_unavailable_physical_spelling) => root.to_path_buf(),
        };
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(_unavailable_physical_spelling) => path.to_path_buf(),
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            return Ok(());
        };
        relative.to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| ScanError::PathNotUtf8 {
                    path: relative.clone(),
                })?;
                parts.push(part.to_owned());
            }
            Component::CurDir => {}
            _ => return Ok(()),
        }
    }
    if !parts.is_empty() {
        relative_paths.push(parts.join("/"));
    }
    Ok(())
}

fn exact_path(path: &Path) -> Result<String, ScanError> {
    path.as_os_str()
        .to_str()
        .map(|text| text.replace('\\', "/"))
        .ok_or_else(|| ScanError::PathNotUtf8 {
            path: path.to_path_buf(),
        })
}
