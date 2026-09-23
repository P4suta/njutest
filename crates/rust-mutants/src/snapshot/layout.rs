// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where the tree and what it reads beside itself are placed inside a snapshot.

use std::path::{Component, Path, PathBuf};

use super::{SnapshotError, SnapshotErrorKind};

/// The source root and every directory it reads outside itself, under the one ancestor they share.
///
/// A copy places everything it holds by substituting one prefix: the ancestor becomes the stage.
/// A single prefix substitution preserves every relative path between the things it moves, because a relative path is a count of components and the substitution changes none of them.
/// That is the whole reason this type exists: a copy that placed the tree and its neighbours by two rules would preserve the paths between them only when the two rules happened to agree, which is what a declaration climbing out of the tree used to discover inside cargo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    ancestor: PathBuf,
    root: PathBuf,
    beside: Vec<PathBuf>,
}

/// One directory a copy reproduces, and where it puts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    source: PathBuf,
    destination: PathBuf,
}

/// A [`Layout`] bound to the directory a copy is made in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    stage: PathBuf,
    source_root: PathBuf,
    root: PathBuf,
    beside: Vec<Placed>,
}

impl Layout {
    /// The layout of `root` and the directories a run may read outside it.
    ///
    /// # Errors
    /// A path that is not absolute or still climbs, an allowed directory that shares no ancestor with the root, and one that is the root or holds it.
    pub fn plan(root: &Path, allowed: &[PathBuf]) -> Result<Self, SnapshotError> {
        let root = settled(root, SnapshotErrorKind::SourceRoot)?;
        let mut beside: Vec<PathBuf> = Vec::new();
        for directory in allowed {
            let directory = settled(directory, SnapshotErrorKind::Layout)?;
            if root.starts_with(&directory) {
                return Err(refused(
                    &directory,
                    "an allowed directory that is the tree, or holds it, would be copied twice",
                ));
            }
            if directory.starts_with(&root) {
                return Err(refused(
                    &directory,
                    "an allowed directory inside the tree is already in the copy",
                ));
            }
            beside.push(directory);
        }
        beside.sort();
        beside.dedup();
        let held = beside.clone();
        beside.retain(|directory| {
            !held
                .iter()
                .any(|other| other != directory && directory.starts_with(other))
        });
        let mut ancestor = root.clone();
        for directory in &beside {
            ancestor = shared(&ancestor, directory).ok_or_else(|| {
                refused(
                    directory,
                    "an allowed directory on another filesystem root shares no place with the \
                     tree, so no copy can hold both where they are",
                )
            })?;
        }
        Ok(Self {
            ancestor,
            root,
            beside,
        })
    }

    /// The tree this is the layout of.
    #[must_use]
    pub fn source_root(&self) -> &Path {
        &self.root
    }

    /// This layout, with the directory a copy of it is made in.
    #[must_use]
    pub fn under(&self, stage: PathBuf) -> Placement {
        let placed = |source: &Path| stage.join(relative(&self.ancestor, source));
        Placement {
            source_root: self.root.clone(),
            root: placed(&self.root),
            beside: self
                .beside
                .iter()
                .map(|source| Placed {
                    source: source.clone(),
                    destination: placed(source),
                })
                .collect(),
            stage,
        }
    }
}

impl Placed {
    /// The directory on disk that is copied.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Where the copy of it goes.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

impl Placement {
    /// The directory the copy is made in, which holds everything it places and nothing else.
    #[must_use]
    pub fn stage(&self) -> &Path {
        &self.stage
    }

    /// Where the measured tree is copied to.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The tree on disk this is a placement of.
    #[must_use]
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Every directory copied beside the tree, in path order.
    #[must_use]
    pub fn beside(&self) -> &[Placed] {
        &self.beside
    }

    /// The directories between the stage and what it holds, each named once, shallowest first.
    ///
    /// The copy creates each of these and nothing else, so exclusive creation stays the rule: a directory already there is a fact to react to rather than one to paper over.
    #[must_use]
    pub fn scaffolding(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = Vec::new();
        let destinations =
            std::iter::once(self.root.as_path()).chain(self.beside.iter().map(Placed::destination));
        for destination in destinations {
            let below = relative(&self.stage, destination);
            let parts: Vec<Component<'_>> = below.components().collect();
            for depth in 0..parts.len() {
                let mut path = self.stage.clone();
                for part in parts.iter().take(depth) {
                    path.push(part);
                }
                found.push(path);
            }
        }
        found.sort();
        found.dedup();
        found.sort_by_key(|path| path.components().count());
        found
    }
}

/// `path` as an absolute path with nothing left to resolve.
fn settled(path: &Path, kind: SnapshotErrorKind) -> Result<PathBuf, SnapshotError> {
    if !path.is_absolute() {
        return Err(SnapshotError::new(
            kind,
            path.display().to_string(),
            "a relative path names no place a copy can reproduce",
        ));
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(SnapshotError::new(
            kind,
            path.display().to_string(),
            "a path that still climbs names no one place, so a copy cannot say where it put it",
        ));
    }
    Ok(path.to_path_buf())
}

/// The longest run of components `one` and `other` begin with, when they begin with any.
fn shared(one: &Path, other: &Path) -> Option<PathBuf> {
    let mut found = PathBuf::new();
    for (left, right) in one.components().zip(other.components()) {
        if left != right {
            break;
        }
        found.push(left);
    }
    (found.components().next().is_some()).then_some(found)
}

/// The components of `path` below `base`, which is a prefix of it.
fn relative(base: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(base) {
        Ok(below) => below.to_path_buf(),
        Err(_base_is_a_prefix_by_construction) => PathBuf::new(),
    }
}

/// A layout refusal naming the path it is about.
fn refused(path: &Path, why: &str) -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::Layout,
        path.display().to_string(),
        why.to_owned(),
    )
}
