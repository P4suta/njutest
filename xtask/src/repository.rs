// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The repository as a gate reads it: what git lists, and never what a build, a run or a report left beside it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::gates::{GateError, REDIRECTING_GIT};

/// Why the repository could not be listed as a gate reads it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ListingError {
    /// git could not list what `dir` holds.
    #[error("git could not list what {} holds: {said}", dir.display())]
    Unlisted {
        /// The directory asked about.
        dir: PathBuf,
        /// What git or the operating system said.
        said: String,
    },
    /// git listed a path under `dir` that is not UTF-8.
    #[error("git listed a path under {} that is not UTF-8", dir.display())]
    NotUtf8 {
        /// The directory asked about.
        dir: PathBuf,
    },
    /// The repository holds a symbolic link.
    #[error("{} is a symbolic link; a gate does not follow a name that can hide or escape the tree it proves", path.display())]
    Symlink {
        /// The link.
        path: PathBuf,
    },
    /// A path git listed could not be read, or copied where it was asked to go.
    #[error("{}: {source}", path.display())]
    Unreadable {
        /// The path.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
}

impl crate::error::Coded for ListingError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Unlisted { .. } => crate::error::XtCode::RepositoryUnlisted,
            Self::NotUtf8 { .. } => crate::error::XtCode::RepositoryPath,
            Self::Symlink { .. } => crate::error::XtCode::RepositorySymlink,
            Self::Unreadable { .. } => crate::error::XtCode::RepositoryUnreadable,
        }
    }
}

impl From<ListingError> for GateError {
    fn from(error: ListingError) -> Self {
        Self(format!("walking the repository: {error}"))
    }
}

/// Copies every file of the repository at `from` to the same relative path under `to`.
///
/// # Errors
/// What [`files`] refuses, or a file that could not be copied.
pub fn copy(from: &Path, to: &Path) -> Result<(), ListingError> {
    for relative in files(from)? {
        let target = to.join(&relative);
        match target.parent() {
            Some(parent) => std::fs::create_dir_all(parent),
            None => Ok(()),
        }
        .and_then(|()| std::fs::copy(from.join(&relative), &target).map(|_bytes| ()))
        .map_err(|source| ListingError::Unreadable {
            path: target,
            source,
        })?;
    }
    Ok(())
}

/// Every file of the repository at `root`, by its workspace-relative slash path, in byte order: what git tracks and what it would track once added, less what the working tree has deleted.
///
/// # Errors
/// git cannot list the tree, lists a path that is not UTF-8, or lists a symbolic link, which a gate does not follow since a name that can point anywhere proves nothing about the tree.
pub fn files(root: &Path) -> Result<Vec<String>, ListingError> {
    let held = listed(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let deleted: BTreeSet<String> = listed(root, &["ls-files", "-z", "--deleted"])?
        .into_iter()
        .collect();
    let mut files: Vec<String> = Vec::new();
    for path in held.into_iter().filter(|path| !deleted.contains(path)) {
        let whole = root.join(&path);
        let metadata =
            std::fs::symlink_metadata(&whole).map_err(|source| ListingError::Unreadable {
                path: whole.clone(),
                source,
            })?;
        if metadata.file_type().is_symlink() {
            return Err(ListingError::Symlink { path: whole });
        }
        files.push(path);
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Every file of the repository at `root` under `base`, a workspace-relative directory, joined to `root`.
///
/// # Errors
/// The failures of [`files`].
pub fn under(root: &Path, base: &str) -> Result<Vec<PathBuf>, ListingError> {
    let prefix = format!("{}/", base.trim_end_matches('/'));
    Ok(files(root)?
        .into_iter()
        .filter(|path| path.starts_with(&prefix))
        .map(|path| root.join(path))
        .collect())
}

/// Whether the workspace-relative `path` names a file with the extension `wanted`.
#[must_use]
pub fn extension_is(path: &str, wanted: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension == wanted)
}

/// Makes `dir` a repository of its own, so a tree laid for a check is read as the repository is: through what git lists.
///
/// # Errors
/// git could not be run, or refused.
pub fn init(dir: &Path) -> std::io::Result<()> {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(dir).args(["init", "--quiet"]);
    for variable in REDIRECTING_GIT {
        git.env_remove(variable);
    }
    let status = git.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "git init in {} ended {status}",
            dir.display()
        )))
    }
}

/// Every entry of `dir`, by its full path in byte order.
///
/// Where the directory holds a repository's files, the entries are what git lists there, since build output beside them is no part of it; where it holds none, as a recording, a scratch or lock directory, or an ignored run does, they are what it holds.
///
/// # Errors
/// The directory cannot be listed.
pub fn entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let held = match listed(
        dir,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            ".",
        ],
    ) {
        Ok(held) => held,
        Err(_in_no_repository) => Vec::new(),
    };
    let mut found: Vec<PathBuf> = if held.is_empty() {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            found.push(entry?.path());
        }
        found
    } else {
        held.iter()
            .map(|path| match path.split_once('/') {
                Some((child, _rest)) => dir.join(child),
                None => dir.join(path),
            })
            .collect()
    };
    found.sort();
    found.dedup();
    Ok(found)
}

/// What `git -C dir <arguments>` lists, one NUL-terminated path at a time.
fn listed(dir: &Path, arguments: &[&str]) -> Result<Vec<String>, ListingError> {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(dir).args(arguments);
    for variable in REDIRECTING_GIT {
        git.env_remove(variable);
    }
    let output = git.output().map_err(|error| ListingError::Unlisted {
        dir: dir.to_path_buf(),
        said: error.to_string(),
    })?;
    if !output.status.success() {
        let said = match String::from_utf8(output.stderr) {
            Ok(said) => said.trim().to_owned(),
            Err(_not_text) => "its diagnostics are not text".to_owned(),
        };
        return Err(ListingError::Unlisted {
            dir: dir.to_path_buf(),
            said,
        });
    }
    paths(dir, output.stdout)
}

/// The NUL-terminated paths git printed for `dir`, each as exact text.
///
/// # Errors
/// [`ListingError::NotUtf8`] when git printed a path that is not UTF-8.
pub fn paths(dir: &Path, printed: Vec<u8>) -> Result<Vec<String>, ListingError> {
    let text = String::from_utf8(printed).map_err(|_not_text| ListingError::NotUtf8 {
        dir: dir.to_path_buf(),
    })?;
    Ok(text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_listed_path_that_is_not_utf8_is_a_typed_refusal() {
        let printed = vec![b'b', 0xff, b'd', 0];
        let refused = super::paths(std::path::Path::new("fixtures"), printed);
        assert!(
            matches!(refused, Err(super::ListingError::NotUtf8 { .. })),
            "a path no protocol a gate feeds could spell is refused, not skipped: {refused:?}"
        );
    }
}
