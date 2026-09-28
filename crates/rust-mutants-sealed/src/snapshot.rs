// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A read-only filesystem snapshot: files and directories by relative path, addressed by the digest of what they hold.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::digest::{Encoder, SealedDigest};
use crate::error::{SealedError, SnapshotFault};

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
        let mut draft = Draft::Directory(BTreeMap::new());
        for (path, entry) in self.entries {
            draft.place(&path, entry)?;
        }
        let mut layout = Layout {
            nodes: Vec::new(),
            encoder: Encoder::new("rust-mutants-sealed/snapshot/v1"),
        };
        draft.lay_out((ROOT, ROOT_INODE), "", &mut layout);
        Ok(Snapshot {
            nodes: layout.nodes.into(),
            digest: layout.encoder.finish(),
        })
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
    /// A file with these bytes.
    File(Vec<u8>),
    /// A directory with these entries.
    Directory(BTreeMap<String, Self>),
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
            let Self::Directory(entries) = here else {
                return Err(conflict());
            };
            if names.peek().is_some() {
                here = entries
                    .entry(name.to_owned())
                    .or_insert_with(|| Self::Directory(BTreeMap::new()));
                continue;
            }
            let placed = match entry {
                Given::File(contents) => Self::File(contents),
                Given::Directory => Self::Directory(BTreeMap::new()),
            };
            return match entries.insert(name.to_owned(), placed) {
                None => Ok(()),
                Some(Self::Directory(_) | Self::File(_)) => Err(conflict()),
            };
        }
        Err(conflict())
    }

    /// Lays this node out under `parent` with the inode `inode`, its children after it, and adds it to the digest.
    fn lay_out(self, (parent, inode): (NodeId, u64), path: &str, layout: &mut Layout) {
        let id = layout.nodes.len();
        match self {
            Self::File(contents) => {
                layout
                    .encoder
                    .tag(b'F')
                    .text(path)
                    .digest(&SealedDigest::of(&contents));
                layout.nodes.push(Node {
                    body: Body::File(contents.into()),
                    inode,
                    parent,
                });
            }
            Self::Directory(children) => {
                layout.encoder.tag(b'D').text(path).count(children.len());
                layout.nodes.push(Node {
                    body: Body::Directory(Arc::new(BTreeMap::new())),
                    inode,
                    parent,
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
