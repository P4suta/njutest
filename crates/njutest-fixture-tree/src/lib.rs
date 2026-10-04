// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Filesystem policy shared by the engine, gates and tests: a fixture tree classified from its immediate entries, and when a file's stamp may stand for its content.

#![forbid(unsafe_code)]

pub mod settled;

use std::path::{Path, PathBuf};

/// One immediate child of a fixture directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureEntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
}

/// A child named by the reader that enumerates a fixture directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureEntry {
    /// One UTF-8 path component.
    pub name: String,
    /// Whether the child is a file or a directory.
    pub kind: FixtureEntryKind,
}

/// A fixture group could not be enumerated or contains a file instead of another group or fixture.
#[derive(Debug)]
pub enum FixtureDiscoveryError<E> {
    /// The reader could not enumerate this directory.
    Read {
        /// The directory being read.
        path: PathBuf,
        /// Why it could not be read.
        source: E,
    },
    /// A file lies outside every fixture.
    NotAFixture {
        /// The file belonging to no fixture.
        path: PathBuf,
        /// The group holding it.
        group: PathBuf,
    },
}

/// Find fixtures through the caller's directory reader, with the same recursive classification for gates and tests.
///
/// A directory holding `Cargo.toml` is a fixture, and every other directory is a group.
/// The root may also hold its README; a file directly inside any group is a refusal.
///
/// # Errors
/// The reader could not enumerate a group, or a group holds a file instead of a fixture.
pub fn discover_fixtures<E>(
    root: &Path,
    mut entries: impl FnMut(&Path) -> Result<Vec<FixtureEntry>, E>,
) -> Result<Vec<String>, FixtureDiscoveryError<E>> {
    fn visit<E>(
        dir: &Path,
        relative: &str,
        entries: &mut impl FnMut(&Path) -> Result<Vec<FixtureEntry>, E>,
        found: &mut Vec<String>,
    ) -> Result<(), FixtureDiscoveryError<E>> {
        let mut children = entries(dir).map_err(|source| FixtureDiscoveryError::Read {
            path: dir.to_path_buf(),
            source,
        })?;
        children.sort_by(|left, right| left.name.cmp(&right.name));
        if !relative.is_empty()
            && children
                .iter()
                .any(|entry| entry.name == "Cargo.toml" && entry.kind == FixtureEntryKind::File)
        {
            found.push(relative.to_owned());
            return Ok(());
        }
        for child in children {
            let path = dir.join(&child.name);
            match child.kind {
                FixtureEntryKind::File if relative.is_empty() && child.name == "README.md" => {}
                FixtureEntryKind::File => {
                    return Err(FixtureDiscoveryError::NotAFixture {
                        path,
                        group: dir.to_path_buf(),
                    });
                }
                FixtureEntryKind::Directory => {
                    let name = if relative.is_empty() {
                        child.name
                    } else {
                        format!("{relative}/{}", child.name)
                    };
                    visit(&path, &name, entries, found)?;
                }
            }
        }
        Ok(())
    }

    let mut found = Vec::new();
    visit(root, "", &mut entries, &mut found)?;
    found.sort();
    Ok(found)
}

/// One filesystem-root spelling for ordinary and Windows extended paths, preserving every real root.
#[must_use]
pub fn filesystem_spelling(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            let mut normalized = match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    PathBuf::from(format!("{}:", char::from(drive).to_ascii_uppercase()))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    let mut name = std::ffi::OsString::from(r"\\");
                    name.push(server);
                    name.push(r"\");
                    name.push(share);
                    PathBuf::from(name)
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => return path.to_path_buf(),
            };
            normalized.extend(path.components().skip(1));
            return normalized;
        }
    }
    path.to_path_buf()
}

#[cfg(all(test, windows))]
mod filesystem_tests {
    use super::filesystem_spelling;
    use std::path::Path;

    #[test]
    fn extended_disk_and_unc_prefixes_name_the_same_roots() {
        for (extended, ordinary) in [
            (r"\\?\c:\trees\root", r"C:\trees\root"),
            (r"\\?\UNC\server\share\root", r"\\server\share\root"),
        ] {
            assert_eq!(
                filesystem_spelling(Path::new(extended)),
                Path::new(ordinary)
            );
        }
    }

    #[test]
    fn different_disks_and_unc_shares_remain_different_roots() {
        for (one, other) in [
            (r"\\?\C:\root", r"D:\root"),
            (r"\\?\UNC\server\one\root", r"\\server\two\root"),
        ] {
            assert_ne!(
                filesystem_spelling(Path::new(one)),
                filesystem_spelling(Path::new(other))
            );
        }
    }
}
