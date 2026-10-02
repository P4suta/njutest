// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A read-only filesystem snapshot: files and directories by relative path, addressed by the digest of what they hold.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::digest::{Encoder, SealedDigest};
use crate::error::{SealedError, SnapshotFault};
use crate::transcript::OverlayState;

/// The identity of a node within one arena.
pub(crate) type NodeId = usize;

/// The node every snapshot and every tree grown from one is rooted at.
pub(crate) const ROOT: NodeId = 0;

/// What a node of a snapshot holds.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    /// A file's bytes, shared by every invocation that reads them.
    File(Arc<[u8]>),
    /// A directory's entries by name.
    Directory(Arc<BTreeMap<String, NodeId>>),
}

/// One file or directory of a snapshot.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// What it holds.
    pub(crate) body: Body,
    /// Its inode number, derived from its path.
    pub(crate) inode: u64,
    /// The directory holding it; the root holds itself.
    pub(crate) parent: NodeId,
    /// Its access and modification times, where the snapshot gave any; a node the instance dates itself holds none.
    pub(crate) times: Option<Times>,
}

/// The access and modification times of a node, in nanoseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Times {
    /// When it was last read.
    pub(crate) accessed: u64,
    /// When it was last written.
    pub(crate) modified: u64,
}

/// A read-only tree of files and directories a guest may be given at a preopened path.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Every node, the root first, the rest in the order a sorted walk meets them.
    nodes: Arc<[Node]>,
    /// The digest of every path and every byte the tree holds.
    digest: SealedDigest,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}

impl Eq for Snapshot {}

impl Snapshot {
    /// Starts a snapshot holding nothing but its root directory.
    #[must_use]
    pub const fn builder() -> SnapshotBuilder {
        SnapshotBuilder {
            entries: BTreeMap::new(),
        }
    }

    /// The digest of every path and every byte the snapshot holds.
    #[must_use]
    pub const fn digest(&self) -> &SealedDigest {
        &self.digest
    }

    /// Every node, the root first.
    pub(crate) fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The directory `names` walk to from the root, none where one of them is not a directory the snapshot holds.
    pub(crate) fn directory(&self, names: &[&str]) -> Option<NodeId> {
        let mut at = ROOT;
        for name in names {
            match &self.nodes.get(at)?.body {
                Body::Directory(entries) => at = *entries.get(*name)?,
                Body::File(_) => return None,
            }
        }
        match self.nodes.get(at)?.body {
            Body::Directory(_) => Some(at),
            Body::File(_) => None,
        }
    }

    /// This snapshot as an invocation's overlay leaves it: each change a path below the root, `/`-separated and empty for the root itself, and what it holds after, in the order an overlay lists them, each directory before what it holds.
    ///
    /// # Errors
    /// [`SealedError::SnapshotPath`] for a path that is not one of named components, a change whose directory the snapshot does not hold by then, or the root changed into anything but a directory.
    pub fn after<'a>(
        &self,
        changes: impl IntoIterator<Item = (&'a str, &'a OverlayState)>,
    ) -> Result<Self, SealedError> {
        let mut draft = self.draft(ROOT).ok_or(SealedError::SnapshotPath {
            path: String::new(),
            fault: SnapshotFault::NoParent,
        })?;
        for (path, state) in changes {
            draft.changed(path, state)?;
        }
        Ok(Self::laid(draft))
    }

    /// The node `node` and everything under it, as a draft.
    fn draft(&self, node: NodeId) -> Option<Draft> {
        let held = self.nodes.get(node)?;
        Some(match &held.body {
            Body::File(bytes) => Draft::File(bytes.to_vec(), held.times),
            Body::Directory(entries) => Draft::Directory(
                entries
                    .iter()
                    .map(|(name, child)| Some((name.clone(), self.draft(*child)?)))
                    .collect::<Option<_>>()?,
                held.times,
            ),
        })
    }

    /// The snapshot `draft` lays out.
    fn laid(draft: Draft) -> Self {
        let mut layout = Layout {
            nodes: Vec::new(),
            encoder: Encoder::new("rust-mutants-sealed/snapshot/v3"),
        };
        draft.lay_out((ROOT, ROOT_INODE), "", &mut layout);
        Self {
            nodes: layout.nodes.into(),
            digest: layout.encoder.finish(),
        }
    }
}

/// What a path given to a snapshot being built names.
#[derive(Debug, Clone)]
enum Given {
    /// A file with these bytes.
    File(Vec<u8>),
    /// A directory, named on its own so it can be empty.
    Directory,
}

/// A snapshot being assembled: each file by its relative path, directories implied by what they hold or named on their own.
#[derive(Debug, Clone)]
pub struct SnapshotBuilder {
    /// Every path given, with what it names.
    entries: BTreeMap<String, Given>,
}

impl SnapshotBuilder {
    /// Adds a file at `path`, a relative path of `/`-separated names.
    ///
    /// # Errors
    /// [`SealedError::SnapshotPath`] for a path that is not relative, or one already given.
    pub fn file(self, path: &str, contents: Vec<u8>) -> Result<Self, SealedError> {
        self.with(path, Given::File(contents))
    }

    /// Adds a directory at `path`, which may then stay empty.
    ///
    /// # Errors
    /// [`SealedError::SnapshotPath`] for a path that is not relative, or one already given.
    pub fn directory(self, path: &str) -> Result<Self, SealedError> {
        self.with(path, Given::Directory)
    }

    /// Adds one entry, refusing a path that is not a relative path of names or that was given before.
    fn with(mut self, path: &str, entry: Given) -> Result<Self, SealedError> {
        let refuse = |fault| SealedError::SnapshotPath {
            path: path.to_owned(),
            fault,
        };
        if !relative_names(path) {
            return Err(refuse(SnapshotFault::NotRelative));
        }
        if self.entries.insert(path.to_owned(), entry).is_some() {
            return Err(refuse(SnapshotFault::Repeated));
        }
        Ok(self)
    }

    /// The snapshot, every directory a path passes through made on the way.
    ///
    /// # Errors
    /// [`SealedError::SnapshotPath`] where a path is a file in one place and a directory in another.
    pub fn build(self) -> Result<Snapshot, SealedError> {
        let mut draft = Draft::Directory(BTreeMap::new(), None);
        for (path, entry) in self.entries {
            draft.place(&path, entry)?;
        }
        Ok(Snapshot::laid(draft))
    }
}

/// An arena being laid out, and the digest being taken of it.
struct Layout {
    /// The nodes so far.
    nodes: Vec<Node>,
    /// The encoding of every node so far.
    encoder: Encoder,
}

/// A snapshot as a nested tree, before it is laid out in an arena.
#[derive(Debug)]
enum Draft {
    /// A file with these bytes and times, where any were given.
    File(Vec<u8>, Option<Times>),
    /// A directory with these entries and times, where any were given.
    Directory(BTreeMap<String, Self>, Option<Times>),
}

impl Draft {
    /// Puts `entry` at `path` under this directory, making every directory on the way.
    fn place(&mut self, path: &str, entry: Given) -> Result<(), SealedError> {
        let conflict = || SealedError::SnapshotPath {
            path: path.to_owned(),
            fault: SnapshotFault::FileAndDirectory,
        };
        let mut names = path.split('/').peekable();
        let mut here = self;
        while let Some(name) = names.next() {
            let Self::Directory(entries, _) = here else {
                return Err(conflict());
            };
            if names.peek().is_some() {
                here = entries
                    .entry(name.to_owned())
                    .or_insert_with(|| Self::Directory(BTreeMap::new(), None));
                continue;
            }
            let placed = match entry {
                Given::File(contents) => Self::File(contents, None),
                Given::Directory => Self::Directory(BTreeMap::new(), None),
            };
            return match entries.insert(name.to_owned(), placed) {
                None => Ok(()),
                Some(Self::Directory(..) | Self::File(..)) => Err(conflict()),
            };
        }
        Err(conflict())
    }

    /// Makes `path`, below this directory and empty for the directory itself, hold what `state` says, its directory already held.
    fn changed(&mut self, path: &str, state: &OverlayState) -> Result<(), SealedError> {
        let refuse = |fault| SealedError::SnapshotPath {
            path: path.to_owned(),
            fault,
        };
        if path.is_empty() {
            return match (self, state) {
                (Self::Directory(_, times), OverlayState::Directory { accessed, modified }) => {
                    *times = Some(Times {
                        accessed: *accessed,
                        modified: *modified,
                    });
                    Ok(())
                }
                (
                    Self::Directory(..) | Self::File(..),
                    OverlayState::Directory { .. }
                    | OverlayState::File { .. }
                    | OverlayState::Removed,
                ) => Err(refuse(SnapshotFault::FileAndDirectory)),
            };
        }
        if !relative_names(path) {
            return Err(refuse(SnapshotFault::NotRelative));
        }
        let (directory, name) = match path.rsplit_once('/') {
            Some((directory, name)) => (Some(directory), name),
            None => (None, path),
        };
        let mut here = self;
        for step in directory
            .into_iter()
            .flat_map(|directory| directory.split('/'))
        {
            let Self::Directory(entries, _) = here else {
                return Err(refuse(SnapshotFault::NoParent));
            };
            here = entries
                .get_mut(step)
                .ok_or_else(|| refuse(SnapshotFault::NoParent))?;
        }
        let Self::Directory(entries, _) = here else {
            return Err(refuse(SnapshotFault::NoParent));
        };
        match state {
            OverlayState::File {
                contents,
                accessed,
                modified,
            } => {
                let times = Some(Times {
                    accessed: *accessed,
                    modified: *modified,
                });
                entries.insert(name.to_owned(), Self::File(contents.clone(), times));
            }
            OverlayState::Directory { accessed, modified } => {
                let times = Some(Times {
                    accessed: *accessed,
                    modified: *modified,
                });
                match entries.get_mut(name) {
                    Some(Self::Directory(_, held)) => *held = times,
                    Some(Self::File(..)) | None => {
                        entries.insert(name.to_owned(), Self::Directory(BTreeMap::new(), times));
                    }
                }
            }
            OverlayState::Removed => {
                entries.remove(name);
            }
        }
        Ok(())
    }

    /// Lays this node out under `parent` with the inode `inode`, its children after it, and adds it to the digest.
    fn lay_out(self, (parent, inode): (NodeId, u64), path: &str, layout: &mut Layout) {
        let id = layout.nodes.len();
        match self {
            Self::File(contents, times) => {
                let encoded = layout
                    .encoder
                    .tag(b'F')
                    .text(path)
                    .digest(&SealedDigest::of(&contents));
                Self::encoded_times(encoded, times);
                layout.nodes.push(Node {
                    body: Body::File(contents.into()),
                    inode,
                    parent,
                    times,
                });
            }
            Self::Directory(children, times) => {
                let encoded = layout.encoder.tag(b'D').text(path).count(children.len());
                Self::encoded_times(encoded, times);
                layout.nodes.push(Node {
                    body: Body::Directory(Arc::new(BTreeMap::new())),
                    inode,
                    parent,
                    times,
                });
                let mut entries = BTreeMap::new();
                for (name, child) in children {
                    let child_path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}/{name}")
                    };
                    entries.insert(name.clone(), layout.nodes.len());
                    child.lay_out((id, inode_of(inode, &name)), &child_path, layout);
                }
                if let Some(node) = layout.nodes.get_mut(id) {
                    node.body = Body::Directory(Arc::new(entries));
                }
            }
        }
    }

    /// Adds `times` to `encoded`, a node without any and one with some encoded so they cannot read as each other.
    fn encoded_times(encoded: &mut Encoder, times: Option<Times>) {
        match times {
            Some(times) => {
                encoded
                    .tag(b'T')
                    .number(times.accessed)
                    .number(times.modified);
            }
            None => {
                encoded.tag(b'N');
            }
        }
    }
}

/// The inode number of every root.
pub(crate) const ROOT_INODE: u64 = 1;

/// The inode number of the entry `name` of the directory whose inode is `parent`: a digest of the path, so it is the same in every invocation.
pub(crate) fn inode_of(parent: u64, name: &str) -> u64 {
    let mut encoder = Encoder::new("rust-mutants-sealed/inode/v1");
    encoder.number(parent).text(name);
    encoder.finish().leading_number()
}

/// Whether `path` is a relative path of names: no empty, `.`, `..` or NUL-bearing component, and not absolute.
fn relative_names(path: &str) -> bool {
    !path.is_empty()
        && path
            .split('/')
            .all(|name| !name.is_empty() && name != "." && name != ".." && !name.contains('\0'))
}
