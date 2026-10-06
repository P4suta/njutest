// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a guest's build spells a path into one tree, decided by how it spelled the tree's root.

/// The rules a path into one tree is read by: those of the system whose build spelled the tree's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Spelling {
    /// A POSIX root, `/…`, or any other that is not a Windows one: `/` separates names, and a path starting with it is absolute.
    Posix,
    /// A Windows root, `X:\…`: `\` and `/` both separate names, a path is read as Windows reads one, and the tree's root is `names` below `drive`.
    Windows {
        /// The drive letter the root is on, as spelled.
        drive: u8,
        /// The names from the drive's root to the tree's.
        names: Vec<String>,
    },
}

/// Where a path given to a directory of a tree starts, and the names it walks from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reading<'path> {
    /// From the directory it was given to.
    Relative(Vec<&'path str>),
    /// From the tree's root: an absolute path below the root of the tree.
    Rooted(Vec<&'path str>),
    /// Nowhere in the tree: an absolute path its root does not begin.
    Elsewhere,
}

impl Spelling {
    /// How the tree preopened at `root` is spelled.
    pub(crate) fn of(root: &str) -> Self {
        match drive_rooted(root) {
            Some((drive, rest)) => Self::Windows {
                drive,
                names: below_drive(rest).into_iter().map(str::to_owned).collect(),
            },
            None => Self::Posix,
        }
    }

    /// Where `path` starts and what it walks.
    pub(crate) fn read<'path>(&self, path: &'path str) -> Reading<'path> {
        match self {
            Self::Posix if path.starts_with('/') => Reading::Elsewhere,
            Self::Posix => {
                Reading::Relative(path.split('/').filter(|name| !name.is_empty()).collect())
            }
            Self::Windows { .. } if path.starts_with(['\\', '/']) => Reading::Elsewhere,
            Self::Windows { drive, names } => match drive_rooted(path) {
                Some((asked, rest)) => {
                    let below = below_drive(rest);
                    let within = asked.eq_ignore_ascii_case(drive)
                        && names.len() <= below.len()
                        && names
                            .iter()
                            .zip(&below)
                            .all(|(root, name)| one_name(root, name));
                    match below.get(names.len()..) {
                        Some(rest) if within => Reading::Rooted(rest.to_vec()),
                        Some(_) | None => Reading::Elsewhere,
                    }
                }
                None if drive_relative(path) => Reading::Elsewhere,
                None => Reading::Relative(relative_names(path)),
            },
        }
    }

    /// The names a path given to the guest's root directory walks below the root of the tree preopened at `root`, which this spells, with how many names that root is below the top of its file system; nothing where the path names no place in the tree.
    ///
    /// wasi-libc gives the root directory an absolute path without its leading `/`, and takes a path a Windows build spelled from a drive's root for a relative one, which it joins onto the directory the guest started in; so a Windows tree reads a path from the last name that begins a drive's root, as Windows reads an absolute path joined onto another, and a POSIX tree reads one whose `..` would climb the machine's directories as none of its own.
    pub(crate) fn reach<'path>(
        &self,
        root: &str,
        path: &'path str,
    ) -> Option<(usize, Vec<&'path str>)> {
        match self {
            Self::Posix => {
                let depth: Vec<&str> = root
                    .split('/')
                    .filter(|name| !name.is_empty() && *name != ".")
                    .collect();
                let mut names = Vec::new();
                for name in path.split('/') {
                    match name {
                        "" | "." => {}
                        ".." => return None,
                        name => names.push(name),
                    }
                }
                let within = depth.len() <= names.len()
                    && depth.iter().zip(&names).all(|(root, name)| root == name);
                if !within {
                    return None;
                }
                let below = names.get(depth.len()..)?;
                Some((depth.len(), below.to_vec()))
            }
            Self::Windows { names: depth, .. } => match self.read(from_last_drive(path)) {
                Reading::Rooted(names) => Some((depth.len(), names)),
                Reading::Relative(_) | Reading::Elsewhere => None,
            },
        }
    }

    /// Whether `path` ends in a separator, which only a directory can.
    pub(crate) fn trailing(&self, path: &str) -> bool {
        match self {
            Self::Posix => path.ends_with('/'),
            Self::Windows { .. } => path.ends_with(['\\', '/']),
        }
    }
}

/// The drive letter of a path spelled from a drive's root, `X:\…` or `X:/…`, and everything after the drive.
fn drive_rooted(path: &str) -> Option<(u8, &str)> {
    match path.as_bytes() {
        [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic() => {
            path.get(2..).map(|rest| (*drive, rest))
        }
        _ => None,
    }
}

/// `path` from the last of its names that begins a drive's root, `X:\` or `X:/`, or all of it where none does.
fn from_last_drive(path: &str) -> &str {
    let mut last = None;
    let mut starts_a_name = true;
    for (at, character) in path.char_indices() {
        if starts_a_name
            && path
                .get(at..)
                .is_some_and(|rest| drive_rooted(rest).is_some())
        {
            last = Some(at);
        }
        starts_a_name = matches!(character, '\\' | '/');
    }
    match last.and_then(|at| path.get(at..)) {
        Some(from) => from,
        None => path,
    }
}

/// Whether two names of a Windows path are one name as NTFS compares names: character by character, each in either case where its case is one other character.
pub(crate) fn one_name(one: &str, other: &str) -> bool {
    one.chars().count() == other.chars().count()
        && one.chars().zip(other.chars()).all(|(left, right)| {
            left == right
                || matches!((upper(left), upper(right)), (Some(left), Some(right)) if left == right)
        })
}

/// The one character `character` is in upper case, where its upper case is one character.
fn upper(character: char) -> Option<char> {
    let mut upper = character.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => Some(single),
        (Some(_), Some(_)) | (None, _) => None,
    }
}

/// Whether `path` names a drive without its root, `X:` or `X:name`, which Windows reads from a directory the guest does not have.
const fn drive_relative(path: &str) -> bool {
    matches!(path.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic())
}

/// The names of a Windows path below its drive's root, as Windows reads them: empty names and `.` dropped, and each `..` taking back the name before it, never past the root.
fn below_drive(rest: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for name in rest.split(['\\', '/']) {
        match name {
            "" | "." => {}
            ".." => {
                names.pop();
            }
            name => names.push(name),
        }
    }
    names
}

/// The names of a relative Windows path, as Windows reads them: empty names and `.` dropped, each `..` taking back the name before it, and those with nothing to take back kept, to climb from where the path starts.
fn relative_names(path: &str) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for name in path.split(['\\', '/']) {
        match name {
            "" | "." => {}
            ".." => match names.last() {
                Some(&last) if last != ".." => {
                    names.pop();
                }
                Some(_) | None => names.push(".."),
            },
            name => names.push(name),
        }
    }
    names
}

#[cfg(test)]
mod tests;
