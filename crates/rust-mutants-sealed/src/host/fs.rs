// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guest's filesystem: each preopened snapshot grown into a tree of this invocation's own, and the descriptors that reach it.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::abi::{
    EVENTRWFLAGS_HANGUP, Errno, FDFLAGS_ALL, FDFLAGS_APPEND, FILETYPE_DIRECTORY,
    FILETYPE_REGULAR_FILE, FILETYPE_UNKNOWN, FSTFLAGS_ATIM, FSTFLAGS_ATIM_NOW, FSTFLAGS_MTIM,
    FSTFLAGS_MTIM_NOW, OFLAGS_ALL, OFLAGS_CREAT, OFLAGS_DIRECTORY, OFLAGS_EXCL, OFLAGS_TRUNC,
    RIGHTS_ALL, RIGHTS_DIRECTORY, RIGHTS_FD_ADVISE, RIGHTS_FD_ALLOCATE, RIGHTS_FD_DATASYNC,
    RIGHTS_FD_FDSTAT_SET_FLAGS, RIGHTS_FD_FILESTAT_GET, RIGHTS_FD_FILESTAT_SET_SIZE,
    RIGHTS_FD_FILESTAT_SET_TIMES, RIGHTS_FD_READ, RIGHTS_FD_READDIR, RIGHTS_FD_SEEK,
    RIGHTS_FD_SYNC, RIGHTS_FD_TELL, RIGHTS_FD_WRITE, RIGHTS_FILE, RIGHTS_PATH_CREATE_DIRECTORY,
    RIGHTS_PATH_CREATE_FILE, RIGHTS_PATH_FILESTAT_GET, RIGHTS_PATH_FILESTAT_SET_SIZE,
    RIGHTS_PATH_FILESTAT_SET_TIMES, RIGHTS_PATH_OPEN, RIGHTS_PATH_READLINK,
    RIGHTS_PATH_REMOVE_DIRECTORY, RIGHTS_PATH_RENAME_SOURCE, RIGHTS_PATH_RENAME_TARGET,
    RIGHTS_PATH_UNLINK_FILE, RIGHTS_POLL_FD_READWRITE, RIGHTS_STDIN, RIGHTS_STDOUT, WHENCE_CUR,
    WHENCE_END, WHENCE_SET,
};
use crate::invocation::{Laid, Preopens, ROOT_NAME};
use crate::snapshot::{Body, NodeId, ROOT, Snapshot, inode_of};
use crate::spelling::{Reading, Spelling, one_name};
use crate::transcript::{OverlayEntry, RefusalReason};

use super::overlay::Walk;

/// What a name the guest makes costs the overlay, besides its bytes.
const NAME_COST: u64 = 64;

/// The most descriptors a guest may hold open at once.
const MOST_DESCRIPTORS: usize = 4096;

/// What a filesystem call failed with: an ordinary answer, or a refusal the transcript records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// An ordinary error number.
    Errno(Errno),
    /// A refusal, answered with its reason's error number and recorded.
    Refused(RefusalReason),
}

/// An error number as a filesystem fault.
const fn errno(errno: Errno) -> Fault {
    Fault::Errno(errno)
}

/// What a call needs of the rights of a descriptor it names, which every way of reaching a descriptor asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Needs {
    /// No right: the call asks about the descriptor table, or changes the table itself.
    Nothing,
    /// Every one of these rights.
    All(u64),
    /// At least one of these rights.
    Any(u64),
}

impl Needs {
    /// What a descriptor holding `rights` is answered with, nothing where they are enough: `badf` where reading or writing is missing, as POSIX answers a descriptor not open for it, and `notcapable` for any other right.
    const fn refusal(self, rights: u64) -> Option<Errno> {
        let missing = match self {
            Self::Nothing => 0,
            Self::All(needed) => needed & !rights,
            Self::Any(needed) => {
                if needed & rights == 0 {
                    needed
                } else {
                    0
                }
            }
        };
        if missing == 0 {
            None
        } else if missing & (RIGHTS_FD_READ | RIGHTS_FD_WRITE) != 0 {
            Some(Errno::Badf)
        } else {
            Some(Errno::Notcapable)
        }
    }

    /// Whether `descriptor` holds what the call needs, and the error number it is answered with where it does not.
    const fn held_by(self, descriptor: &Descriptor) -> Result<(), Errno> {
        match self.refusal(descriptor.rights) {
            None => Ok(()),
            Some(refused) => Err(refused),
        }
    }
}

/// What a sync asks of the file it flushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    /// `fd_datasync`: the data.
    Data,
    /// `fd_sync`: the data and the metadata.
    Everything,
}

/// A file's bytes: the snapshot's until the guest writes them, the overlay's after.
#[derive(Debug, Clone)]
pub(crate) enum Contents {
    /// The snapshot's bytes, shared and untouched.
    Snapshot(Arc<[u8]>),
    /// The bytes the guest wrote, which the overlay holds and counts.
    Overlay(Vec<u8>),
}

impl Contents {
    /// The bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::Snapshot(bytes) => bytes,
            Self::Overlay(bytes) => bytes,
        }
    }
}

/// What a node holds for this invocation.
#[derive(Debug, Clone)]
pub(crate) enum Held {
    /// A file.
    File(Contents),
    /// A directory's entries by name.
    Directory(Arc<BTreeMap<String, NodeId>>),
}

/// A node as the invocation holds it.
#[derive(Debug, Clone)]
pub(crate) struct Live {
    /// What it holds.
    pub(crate) held: Held,
    /// Its inode number.
    inode: u64,
    /// The directory holding it; a root holds itself.
    parent: NodeId,
    /// Its access time.
    pub(crate) accessed: u64,
    /// Its modification time.
    pub(crate) modified: u64,
}

/// One preopened snapshot, grown for this invocation.
#[derive(Debug)]
struct Tree {
    /// The guest path it is preopened at.
    guest_path: String,
    /// How a path into it is read, as its root is spelled.
    spelling: Spelling,
    /// The snapshot it grew from.
    base: Snapshot,
    /// Every node, the snapshot's first under their own identities, then every node the guest made.
    nodes: Vec<Live>,
}

impl Tree {
    /// The tree `snapshot` grows into, preopened at `guest_path`, read as `spelling` says, and every node of it accessed and modified at `started`.
    fn grown(guest_path: &str, snapshot: &Snapshot, (spelling, started): (&Spelling, u64)) -> Self {
        Self {
            guest_path: guest_path.to_owned(),
            spelling: spelling.clone(),
            base: snapshot.clone(),
            nodes: snapshot
                .nodes()
                .iter()
                .map(|node| Live {
                    held: match &node.body {
                        Body::File(bytes) => Held::File(Contents::Snapshot(Arc::clone(bytes))),
                        Body::Directory(entries) => Held::Directory(Arc::clone(entries)),
                    },
                    inode: node.inode,
                    parent: node.parent,
                    accessed: match node.times {
                        Some(times) => times.accessed,
                        None => started,
                    },
                    modified: match node.times {
                        Some(times) => times.modified,
                        None => started,
                    },
                })
                .collect(),
        }
    }
}

/// What a descriptor reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Object {
    /// Standard input, which is always at its end.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// A regular file of a tree, at a position.
    File {
        /// The tree.
        tree: usize,
        /// The node.
        node: NodeId,
        /// Where the next read or write happens.
        position: u64,
    },
    /// A directory of a tree.
    Directory {
        /// The tree.
        tree: usize,
        /// The node.
        node: NodeId,
        /// Whether the guest was given it as its tree's root, named by the tree's guest path, rather than opened it.
        preopened: bool,
    },
    /// The guest's root directory, `/`, which reaches each tree by a path below the tree's root as the tree spells one.
    Root,
}

/// An open regular file: where it is, and where its descriptor stands in it.
#[derive(Debug, Clone, Copy)]
struct Opened {
    /// The tree and the node.
    at: (usize, NodeId),
    /// Where the next read or write happens.
    position: u64,
}

/// An open descriptor.
#[derive(Debug, Clone, Copy)]
struct Descriptor {
    /// What it reaches.
    object: Object,
    /// Its descriptor flags.
    flags: u32,
    /// The rights it carries.
    rights: u64,
    /// The rights descriptors opened through it may carry.
    inheriting: u64,
}

/// Which stream a descriptor that moves bytes is, for the host to route a read or a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stream {
    /// Standard input.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// A regular file.
    File,
}

/// A descriptor's `fdstat`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fdstat {
    /// Its file type.
    pub(crate) filetype: u8,
    /// Its descriptor flags.
    pub(crate) flags: u16,
    /// The rights it carries.
    pub(crate) rights: u64,
    /// The rights descriptors opened through it may carry.
    pub(crate) inheriting: u64,
}

/// A file's `filestat`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Filestat {
    /// The device: one for each preopened tree, zero for the standard streams.
    pub(crate) device: u64,
    /// The inode number.
    pub(crate) inode: u64,
    /// The file type.
    pub(crate) filetype: u8,
    /// The size in bytes.
    pub(crate) size: u64,
    /// The access time.
    pub(crate) accessed: u64,
    /// The modification time, which is also the status change time.
    pub(crate) modified: u64,
}

/// One directory entry as `fd_readdir` lays it out.
#[derive(Debug, Clone)]
pub(crate) struct Dirent {
    /// The cookie of the entry after it.
    pub(crate) next: u64,
    /// Its inode number.
    pub(crate) inode: u64,
    /// Its file type.
    pub(crate) filetype: u8,
    /// Its name.
    pub(crate) name: String,
}

/// What `path_open` asks for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpenRequest {
    /// The open flags.
    pub(crate) oflags: u32,
    /// The rights the new descriptor asks for.
    pub(crate) rights: u64,
    /// The rights descriptors opened through it may carry.
    pub(crate) inheriting: u64,
    /// The new descriptor's flags.
    pub(crate) fdflags: u32,
}

/// What a set-times call asks for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TimesRequest {
    /// The access time to set, where the flags say to set it to a value.
    pub(crate) accessed: u64,
    /// The modification time to set, where the flags say to set it to a value.
    pub(crate) modified: u64,
    /// Which times to set, and whether to a value or to now.
    pub(crate) flags: u32,
    /// What the realtime clock reads now.
    pub(crate) now: u64,
}

/// Which way a poll subscription waits on a descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// For bytes to read.
    Read,
    /// For room to write.
    Write,
}

/// Where a path resolved to.
#[derive(Debug, Clone)]
enum Found {
    /// Something the path names, and the directory entry naming it, where there is one.
    Existing {
        /// The node.
        node: NodeId,
        /// The directory and the name the path reached it by; none for `.`, `..` and the start.
        place: Option<(NodeId, String)>,
    },
    /// Nothing, in a directory that exists.
    Absent {
        /// The directory the last name would be in.
        parent: NodeId,
        /// The last name.
        name: String,
    },
}

/// A resolved path.
#[derive(Debug, Clone)]
struct Resolved {
    /// The tree it is in.
    tree: usize,
    /// What it names.
    found: Found,
    /// Whether it ended in a slash, which only a directory can.
    trailing: bool,
}

/// Every preopened tree of one invocation and every descriptor the guest holds.
#[derive(Debug)]
pub(crate) struct Filesystem {
    /// The trees, in the order they were preopened.
    trees: Vec<Tree>,
    /// Every open descriptor, by number.
    descriptors: BTreeMap<u32, Descriptor>,
    /// How many bytes the overlay holds.
    overlay: u64,
    /// How many bytes the overlay may hold.
    limit: u64,
    /// The time every file and directory is accessed and modified at until the guest sets another: what the realtime clock reads when the invocation starts.
    started: u64,
}

impl Filesystem {
    /// The standard streams at 0, 1 and 2, and each preopen from 3 in order, every node dated `started`.
    pub(crate) fn new(preopens: &Preopens, (limit, started): (u64, u64)) -> Result<Self, Errno> {
        let mut descriptors = BTreeMap::from([
            (0, standard(Object::Stdin, RIGHTS_STDIN)),
            (1, standard(Object::Stdout, RIGHTS_STDOUT)),
            (2, standard(Object::Stderr, RIGHTS_STDOUT)),
        ]);
        let mut trees = Vec::new();
        for (position, laid) in preopens.laid().iter().enumerate() {
            let number = u32::try_from(position)
                .map_err(|_wide| Errno::Mfile)?
                .checked_add(3)
                .ok_or(Errno::Mfile)?;
            let object = match laid {
                Laid::Tree {
                    path,
                    snapshot,
                    spelling,
                } => {
                    trees.push(Tree::grown(path, snapshot, (spelling, started)));
                    let tree = trees.len().checked_sub(1).ok_or(Errno::Mfile)?;
                    Object::Directory {
                        tree,
                        node: ROOT,
                        preopened: true,
                    }
                }
                Laid::Root { .. } => Object::Root,
            };
            descriptors.insert(
                number,
                Descriptor {
                    object,
                    flags: 0,
                    rights: RIGHTS_DIRECTORY,
                    inheriting: RIGHTS_ALL,
                },
            );
        }
        Ok(Self {
            trees,
            descriptors,
            overlay: 0,
            limit,
            started,
        })
    }

    /// The time every file and directory is accessed and modified at until the guest sets another.
    pub(crate) const fn started(&self) -> u64 {
        self.started
    }

    /// The descriptor `fd`, once it is known to hold what the call `needs`.
    fn descriptor(&self, fd: u32, needs: Needs) -> Result<&Descriptor, Errno> {
        let descriptor = self.descriptors.get(&fd).ok_or(Errno::Badf)?;
        needs.held_by(descriptor)?;
        Ok(descriptor)
    }

    /// The descriptor `fd`, to change, once it is known to hold what the call `needs`.
    fn descriptor_mut(&mut self, fd: u32, needs: Needs) -> Result<&mut Descriptor, Errno> {
        let descriptor = self.descriptors.get_mut(&fd).ok_or(Errno::Badf)?;
        needs.held_by(descriptor)?;
        Ok(descriptor)
    }

    /// The node `node` of tree `tree`.
    pub(crate) fn live(&self, tree: usize, node: NodeId) -> Result<&Live, Errno> {
        self.trees
            .get(tree)
            .and_then(|held| held.nodes.get(node))
            .ok_or(Errno::Badf)
    }

    /// The node `node` of tree `tree`, to change.
    fn live_mut(&mut self, tree: usize, node: NodeId) -> Result<&mut Live, Errno> {
        self.trees
            .get_mut(tree)
            .and_then(|held| held.nodes.get_mut(node))
            .ok_or(Errno::Badf)
    }

    /// Which stream `fd` is, once it is known to hold what the call `needs`.
    pub(crate) fn stream(&self, fd: u32, needs: Needs) -> Result<Stream, Errno> {
        let descriptor = self.descriptor(fd, Needs::Nothing)?;
        let stream = match descriptor.object {
            Object::Stdin => Stream::Stdin,
            Object::Stdout => Stream::Stdout,
            Object::Stderr => Stream::Stderr,
            Object::File { .. } => Stream::File,
            Object::Directory { .. } | Object::Root => return Err(Errno::Isdir),
        };
        needs.held_by(descriptor)?;
        Ok(stream)
    }

    /// The file `fd` reaches, and where it stands in it, once it is known to hold what the call `needs`.
    fn file(&self, fd: u32, needs: Needs) -> Result<Opened, Errno> {
        let descriptor = self.descriptor(fd, Needs::Nothing)?;
        let opened = match descriptor.object {
            Object::File {
                tree,
                node,
                position,
            } => Opened {
                at: (tree, node),
                position,
            },
            Object::Directory { .. } | Object::Root => return Err(Errno::Isdir),
            Object::Stdin | Object::Stdout | Object::Stderr => return Err(Errno::Spipe),
        };
        needs.held_by(descriptor)?;
        Ok(opened)
    }

    /// The tree and node of the file `fd` reaches, once it is known to hold what the call `needs`.
    fn file_with(&self, fd: u32, needs: Needs) -> Result<(usize, NodeId), Errno> {
        self.file(fd, needs).map(|opened| opened.at)
    }

    /// The bytes of the file `node` of tree `tree`.
    fn contents(&self, tree: usize, node: NodeId) -> Result<&[u8], Errno> {
        match &self.live(tree, node)?.held {
            Held::File(contents) => Ok(contents.bytes()),
            Held::Directory(_) => Err(Errno::Isdir),
        }
    }

    /// Up to `len` bytes of `fd` at its position, which moves past them.
    pub(crate) fn read(&mut self, fd: u32, len: usize) -> Result<Vec<u8>, Errno> {
        if self.stream(fd, Needs::All(RIGHTS_FD_READ))? == Stream::Stdin {
            return Ok(Vec::new());
        }
        let opened = self.file(fd, Needs::All(RIGHTS_FD_READ))?;
        let (tree, node) = opened.at;
        let bytes = slice_at(self.contents(tree, node)?, opened.position, len).to_vec();
        let read = u64::try_from(bytes.len()).map_err(|_wide| Errno::Overflow)?;
        let past = opened.position.checked_add(read).ok_or(Errno::Overflow)?;
        self.set_position(fd, past)?;
        Ok(bytes)
    }

    /// Up to `len` bytes of `fd` at `offset`, the position left where it was.
    pub(crate) fn pread(&self, fd: u32, len: usize, offset: u64) -> Result<Vec<u8>, Errno> {
        let (tree, node) = self.file_with(fd, Needs::All(RIGHTS_FD_READ | RIGHTS_FD_SEEK))?;
        Ok(slice_at(self.contents(tree, node)?, offset, len).to_vec())
    }

    /// Writes `data` to the file `fd` at its position, or at its end for an appending descriptor, and moves the position past it.
    pub(crate) fn write(&mut self, fd: u32, data: &[u8]) -> Result<usize, Fault> {
        let writing = Needs::All(RIGHTS_FD_WRITE);
        let opened = self.file(fd, writing).map_err(errno)?;
        let (tree, node) = opened.at;
        let at = if self.descriptor(fd, writing).map_err(errno)?.flags & FDFLAGS_APPEND == 0 {
            opened.position
        } else {
            len_of(self.contents(tree, node).map_err(errno)?)?
        };
        let end = self.write_at((tree, node), data, at)?;
        self.set_position(fd, end).map_err(errno)?;
        Ok(data.len())
    }

    /// Writes `data` to the file `fd` at `offset`, the position left where it was.
    pub(crate) fn pwrite(&mut self, fd: u32, data: &[u8], offset: u64) -> Result<usize, Fault> {
        let (tree, node) = self
            .file_with(fd, Needs::All(RIGHTS_FD_WRITE | RIGHTS_FD_SEEK))
            .map_err(errno)?;
        self.write_at((tree, node), data, offset)?;
        Ok(data.len())
    }

    /// Writes `data` into the file `node` of tree `tree` at `at`, zero-filling any gap, and answers where the write ended.
    fn write_at(
        &mut self,
        (tree, node): (usize, NodeId),
        data: &[u8],
        at: u64,
    ) -> Result<u64, Fault> {
        let start = usize::try_from(at).map_err(|_wide| errno(Errno::Fbig))?;
        let end = start.checked_add(data.len()).ok_or(errno(Errno::Fbig))?;
        let held = self.contents(tree, node).map_err(errno)?.len();
        self.resize((tree, node), held.max(end))?;
        self.written(tree, node)
            .map_err(errno)?
            .get_mut(start..end)
            .ok_or(errno(Errno::Fbig))?
            .copy_from_slice(data);
        u64::try_from(end).map_err(|_wide| errno(Errno::Fbig))
    }

    /// The overlay's bytes of the file `node`, which a write has already copied out of the snapshot.
    fn written(&mut self, tree: usize, node: NodeId) -> Result<&mut Vec<u8>, Errno> {
        match &mut self.live_mut(tree, node)?.held {
            Held::File(Contents::Overlay(bytes)) => Ok(bytes),
            Held::File(Contents::Snapshot(_)) | Held::Directory(_) => Err(Errno::Isdir),
        }
    }

    /// Makes the file `node` of tree `tree` `len` bytes long in the overlay, charging the overlay for what it now holds.
    fn resize(&mut self, (tree, node): (usize, NodeId), len: usize) -> Result<(), Fault> {
        let (before, copied) = match &self.live(tree, node).map_err(errno)?.held {
            Held::File(Contents::Overlay(bytes)) => (len_of(bytes)?, None),
            Held::File(Contents::Snapshot(bytes)) => (0, Some(Arc::clone(bytes))),
            Held::Directory(_) => return Err(errno(Errno::Isdir)),
        };
        let after = u64::try_from(len).map_err(|_wide| errno(Errno::Fbig))?;
        let overlay = self
            .overlay
            .checked_sub(before)
            .and_then(|rest| rest.checked_add(after))
            .ok_or(errno(Errno::Fbig))?;
        if overlay > self.limit && after > before {
            return Err(Fault::Refused(RefusalReason::OverlayFull));
        }
        self.overlay = overlay;
        let live = self.live_mut(tree, node).map_err(errno)?;
        if let Some(snapshot) = copied {
            let kept = snapshot.split_at(len.min(snapshot.len())).0;
            live.held = Held::File(Contents::Overlay(kept.to_vec()));
        }
        match &mut live.held {
            Held::File(Contents::Overlay(bytes)) => bytes.resize(len, 0),
            Held::File(Contents::Snapshot(_)) | Held::Directory(_) => {
                return Err(errno(Errno::Isdir));
            }
        }
        Ok(())
    }

    /// Moves the position of the file `fd` to `to`, where the call that moves it has already asked what it needs.
    fn set_position(&mut self, fd: u32, to: u64) -> Result<(), Errno> {
        match &mut self.descriptor_mut(fd, Needs::Nothing)?.object {
            Object::File { position, .. } => {
                *position = to;
                Ok(())
            }
            Object::Directory { .. } | Object::Root => Err(Errno::Isdir),
            Object::Stdin | Object::Stdout | Object::Stderr => Err(Errno::Spipe),
        }
    }

    /// Moves the position of `fd` by `delta` from `whence`, and answers where it now is: asking where it is without moving it needs only the right to ask it.
    pub(crate) fn seek(&mut self, fd: u32, delta: i64, whence: u32) -> Result<u64, Errno> {
        let needs = if delta == 0 && whence == WHENCE_CUR {
            Needs::Any(RIGHTS_FD_TELL | RIGHTS_FD_SEEK)
        } else {
            Needs::All(RIGHTS_FD_SEEK)
        };
        let opened = self.file(fd, needs)?;
        let (tree, node) = opened.at;
        let from = match whence {
            WHENCE_SET => 0,
            WHENCE_CUR => opened.position,
            WHENCE_END => len_of(self.contents(tree, node)?).map_err(|_full| Errno::Overflow)?,
            _ => return Err(Errno::Inval),
        };
        let to = from.checked_add_signed(delta).ok_or(Errno::Inval)?;
        self.set_position(fd, to)?;
        Ok(to)
    }

    /// The position of `fd`.
    pub(crate) fn tell(&self, fd: u32) -> Result<u64, Errno> {
        self.file(fd, Needs::Any(RIGHTS_FD_TELL | RIGHTS_FD_SEEK))
            .map(|opened| opened.position)
    }

    /// The `fdstat` of `fd`.
    pub(crate) fn fdstat(&self, fd: u32) -> Result<Fdstat, Errno> {
        let descriptor = self.descriptor(fd, Needs::Nothing)?;
        let filetype = match descriptor.object {
            Object::Stdin | Object::Stdout | Object::Stderr => FILETYPE_UNKNOWN,
            Object::File { .. } => FILETYPE_REGULAR_FILE,
            Object::Directory { .. } | Object::Root => FILETYPE_DIRECTORY,
        };
        Ok(Fdstat {
            filetype,
            flags: u16::try_from(descriptor.flags).map_err(|_wide| Errno::Inval)?,
            rights: descriptor.rights,
            inheriting: descriptor.inheriting,
        })
    }

    /// Sets the flags of `fd`.
    pub(crate) fn set_flags(&mut self, fd: u32, flags: u32) -> Result<(), Errno> {
        if flags & !FDFLAGS_ALL != 0 {
            return Err(Errno::Inval);
        }
        self.descriptor_mut(fd, Needs::All(RIGHTS_FD_FDSTAT_SET_FLAGS))?
            .flags = flags;
        Ok(())
    }

    /// Narrows the rights of `fd`, refusing to widen them.
    pub(crate) fn set_rights(
        &mut self,
        fd: u32,
        rights: u64,
        inheriting: u64,
    ) -> Result<(), Errno> {
        let descriptor = self.descriptor_mut(fd, Needs::Nothing)?;
        if rights & !descriptor.rights != 0 || inheriting & !descriptor.inheriting != 0 {
            return Err(Errno::Notcapable);
        }
        descriptor.rights = rights;
        descriptor.inheriting = inheriting;
        Ok(())
    }

    /// The `filestat` of what `fd` reaches; the root directory is no place in a tree, so its own is refused.
    pub(crate) fn filestat(&self, fd: u32) -> Result<Filestat, Fault> {
        match self
            .descriptor(fd, Needs::All(RIGHTS_FD_FILESTAT_GET))
            .map_err(errno)?
            .object
        {
            Object::Stdin | Object::Stdout | Object::Stderr => Ok(Filestat {
                device: 0,
                inode: 0,
                filetype: FILETYPE_UNKNOWN,
                size: 0,
                accessed: self.started,
                modified: self.started,
            }),
            Object::File { tree, node, .. } | Object::Directory { tree, node, .. } => {
                self.node_stat(tree, node).map_err(errno)
            }
            Object::Root => Err(Fault::Refused(RefusalReason::Escape)),
        }
    }

    /// The `filestat` of the node `node` of tree `tree`.
    fn node_stat(&self, tree: usize, node: NodeId) -> Result<Filestat, Errno> {
        let live = self.live(tree, node)?;
        let (filetype, size) = match &live.held {
            Held::File(contents) => (
                FILETYPE_REGULAR_FILE,
                len_of(contents.bytes()).map_err(|_full| Errno::Overflow)?,
            ),
            Held::Directory(_) => (FILETYPE_DIRECTORY, 0),
        };
        Ok(Filestat {
            device: u64::try_from(tree)
                .map_err(|_wide| Errno::Overflow)?
                .checked_add(1)
                .ok_or(Errno::Overflow)?,
            inode: live.inode,
            filetype,
            size,
            accessed: live.accessed,
            modified: live.modified,
        })
    }

    /// Makes the file `fd` reaches `size` bytes long.
    pub(crate) fn set_size(&mut self, fd: u32, size: u64) -> Result<(), Fault> {
        let (tree, node) = self
            .file_with(fd, Needs::All(RIGHTS_FD_FILESTAT_SET_SIZE))
            .map_err(errno)?;
        let size = usize::try_from(size).map_err(|_wide| errno(Errno::Fbig))?;
        self.resize((tree, node), size)
    }

    /// Makes the file `fd` reaches at least `offset + len` bytes long.
    pub(crate) fn allocate(&mut self, fd: u32, offset: u64, len: u64) -> Result<(), Fault> {
        let (tree, node) = self
            .file_with(fd, Needs::All(RIGHTS_FD_ALLOCATE))
            .map_err(errno)?;
        let end = offset.checked_add(len).ok_or(errno(Errno::Fbig))?;
        let end = usize::try_from(end).map_err(|_wide| errno(Errno::Fbig))?;
        let held = self.contents(tree, node).map_err(errno)?.len();
        if end > held {
            self.resize((tree, node), end)?;
        }
        Ok(())
    }

    /// Answers whether `fd` is a file advice can be given about, and `advice` advice WASI defines.
    pub(crate) fn advise(&self, fd: u32, advice: u32) -> Result<(), Errno> {
        let descriptor = self.descriptor(fd, Needs::Nothing)?;
        match descriptor.object {
            Object::File { .. } => Needs::All(RIGHTS_FD_ADVISE).held_by(descriptor)?,
            Object::Directory { .. }
            | Object::Root
            | Object::Stdin
            | Object::Stdout
            | Object::Stderr => return Err(Errno::Badf),
        }
        if advice > 5 {
            return Err(Errno::Inval);
        }
        Ok(())
    }

    /// Answers whether `fd` is open with the right to flush what `flush` asks, which is all a flush of memory needs.
    pub(crate) fn sync(&self, fd: u32, flush: Flush) -> Result<(), Errno> {
        let right = match flush {
            Flush::Data => RIGHTS_FD_DATASYNC,
            Flush::Everything => RIGHTS_FD_SYNC,
        };
        self.descriptor(fd, Needs::All(right)).map(|_open| ())
    }

    /// Sets the times of what `fd` reaches; the root directory is no place in a tree, so its own are refused.
    pub(crate) fn set_times(&mut self, fd: u32, request: TimesRequest) -> Result<(), Fault> {
        let descriptor = self.descriptor(fd, Needs::Nothing).map_err(errno)?;
        match descriptor.object {
            Object::Stdin | Object::Stdout | Object::Stderr => Err(errno(Errno::Badf)),
            Object::File { tree, node, .. } | Object::Directory { tree, node, .. } => {
                Needs::All(RIGHTS_FD_FILESTAT_SET_TIMES)
                    .held_by(descriptor)
                    .map_err(errno)?;
                self.touch(tree, node, request).map_err(errno)
            }
            Object::Root => Err(Fault::Refused(RefusalReason::Escape)),
        }
    }

    /// Sets the times of the node `node` as `request` asks.
    fn touch(&mut self, tree: usize, node: NodeId, request: TimesRequest) -> Result<(), Errno> {
        let flags = request.flags;
        if flags & !0xf != 0
            || (flags & FSTFLAGS_ATIM != 0 && flags & FSTFLAGS_ATIM_NOW != 0)
            || (flags & FSTFLAGS_MTIM != 0 && flags & FSTFLAGS_MTIM_NOW != 0)
        {
            return Err(Errno::Inval);
        }
        let live = self.live_mut(tree, node)?;
        if flags & FSTFLAGS_ATIM != 0 {
            live.accessed = request.accessed;
        }
        if flags & FSTFLAGS_ATIM_NOW != 0 {
            live.accessed = request.now;
        }
        if flags & FSTFLAGS_MTIM != 0 {
            live.modified = request.modified;
        }
        if flags & FSTFLAGS_MTIM_NOW != 0 {
            live.modified = request.now;
        }
        Ok(())
    }

    /// Closes `fd`.
    pub(crate) fn close(&mut self, fd: u32) -> Result<(), Errno> {
        self.descriptors
            .remove(&fd)
            .map(|_closed| ())
            .ok_or(Errno::Badf)
    }

    /// Moves the descriptor `from` onto the number `to`, closing what `to` was.
    pub(crate) fn renumber(&mut self, from: u32, to: u32) -> Result<(), Errno> {
        self.descriptor(to, Needs::Nothing)?;
        let moved = self.descriptors.remove(&from).ok_or(Errno::Badf)?;
        self.descriptors.insert(to, moved);
        Ok(())
    }

    /// The name `fd` was preopened by: its tree's guest path, or `/` for the root directory.
    pub(crate) fn preopened(&self, fd: u32) -> Result<&str, Errno> {
        match self.descriptor(fd, Needs::Nothing)?.object {
            Object::Directory {
                tree,
                preopened: true,
                ..
            } => self
                .trees
                .get(tree)
                .map(|held| held.guest_path.as_str())
                .ok_or(Errno::Badf),
            Object::Root => Ok(ROOT_NAME),
            Object::Directory {
                preopened: false, ..
            }
            | Object::File { .. }
            | Object::Stdin
            | Object::Stdout
            | Object::Stderr => Err(Errno::Badf),
        }
    }

    /// The entries of the directory `fd` from `cookie` on: `.`, `..`, then every name in order; the root directory is no place in a tree, so it lists nothing and is refused.
    pub(crate) fn readdir(&self, fd: u32, cookie: u64) -> Result<Vec<Dirent>, Fault> {
        let (tree, node) = self.directory(fd, Needs::All(RIGHTS_FD_READDIR))?;
        let live = self.live(tree, node).map_err(errno)?;
        let Held::Directory(entries) = &live.held else {
            return Err(errno(Errno::Notdir));
        };
        let parent = self.live(tree, live.parent).map_err(errno)?;
        let mut all = vec![
            (".".to_owned(), live.inode, FILETYPE_DIRECTORY),
            ("..".to_owned(), parent.inode, FILETYPE_DIRECTORY),
        ];
        for (name, child) in entries.iter() {
            let child = self.live(tree, *child).map_err(errno)?;
            let filetype = match child.held {
                Held::File(_) => FILETYPE_REGULAR_FILE,
                Held::Directory(_) => FILETYPE_DIRECTORY,
            };
            all.push((name.clone(), child.inode, filetype));
        }
        let skip = usize::try_from(cookie).map_err(|_wide| errno(Errno::Inval))?;
        let mut dirents = Vec::new();
        for (index, (name, inode, filetype)) in all.into_iter().enumerate().skip(skip) {
            let next = u64::try_from(index)
                .map_err(|_wide| errno(Errno::Overflow))?
                .checked_add(1)
                .ok_or(errno(Errno::Overflow))?;
            dirents.push(Dirent {
                next,
                inode,
                filetype,
                name,
            });
        }
        Ok(dirents)
    }

    /// The tree and node of the directory `fd` reaches, refusing the root directory, which is no place in a tree, once it is known to hold what the call `needs`.
    fn directory(&self, fd: u32, needs: Needs) -> Result<(usize, NodeId), Fault> {
        let descriptor = self.descriptor(fd, Needs::Nothing).map_err(errno)?;
        let entered = match descriptor.object {
            Object::Directory { tree, node, .. } => (tree, node),
            Object::Root => return Err(Fault::Refused(RefusalReason::Escape)),
            Object::File { .. } | Object::Stdin | Object::Stdout | Object::Stderr => {
                return Err(errno(Errno::Notdir));
            }
        };
        needs.held_by(descriptor).map_err(errno)?;
        Ok(entered)
    }

    /// The entries of the directory `node` of tree `tree`.
    pub(crate) fn entries(
        &self,
        tree: usize,
        node: NodeId,
    ) -> Result<&BTreeMap<String, NodeId>, Errno> {
        match &self.live(tree, node)?.held {
            Held::Directory(entries) => Ok(entries),
            Held::File(_) => Err(Errno::Notdir),
        }
    }

    /// The entries of the directory `node` of tree `tree`, to change.
    fn entries_mut(
        &mut self,
        tree: usize,
        node: NodeId,
    ) -> Result<&mut BTreeMap<String, NodeId>, Errno> {
        match &mut self.live_mut(tree, node)?.held {
            Held::Directory(entries) => Ok(Arc::make_mut(entries)),
            Held::File(_) => Err(Errno::Notdir),
        }
    }

    /// Whether the node `node` of tree `tree` is a directory.
    fn is_directory(&self, tree: usize, node: NodeId) -> Result<bool, Errno> {
        Ok(matches!(self.live(tree, node)?.held, Held::Directory(_)))
    }

    /// Resolves `path` from the directory `fd` once it is known to hold what the call `needs`, refusing one that leaves what `fd` reaches: its own directory, or from the root directory every tree whose own spelling of an absolute path the path is not.
    fn resolve(&self, fd: u32, path: &str, needs: Needs) -> Result<Resolved, Fault> {
        if path.is_empty() {
            return Err(errno(Errno::Noent));
        }
        if path.contains('\0') {
            return Err(errno(Errno::Inval));
        }
        let descriptor = self.descriptor(fd, Needs::Nothing).map_err(errno)?;
        let (tree, start, names) = match descriptor.object {
            Object::Directory { tree, node, .. } => {
                let spelling = &self.trees.get(tree).ok_or(errno(Errno::Badf))?.spelling;
                match spelling.read(path) {
                    Reading::Relative(names) => (tree, node, names),
                    Reading::Rooted(_) | Reading::Elsewhere => match spelling {
                        Spelling::Windows { .. } => {
                            let (tree, names) = self
                                .rooted(path)
                                .ok_or(Fault::Refused(RefusalReason::Escape))?;
                            (tree, ROOT, names)
                        }
                        Spelling::Posix => {
                            return Err(Fault::Refused(RefusalReason::Escape));
                        }
                    },
                }
            }
            Object::Root => {
                let (tree, names) = self
                    .rooted(path)
                    .ok_or(Fault::Refused(RefusalReason::Escape))?;
                (tree, ROOT, names)
            }
            Object::File { .. } | Object::Stdin | Object::Stdout | Object::Stderr => {
                return Err(errno(Errno::Notdir));
            }
        };
        needs.held_by(descriptor).map_err(errno)?;
        let mut chain = vec![start];
        let mut found = Found::Existing {
            node: start,
            place: None,
        };
        for (at, name) in names.iter().enumerate() {
            let last = at.checked_add(1) == Some(names.len());
            found = self.step(tree, (&found, &mut chain), (name, last))?;
        }
        let trailing = self
            .trees
            .get(tree)
            .ok_or(errno(Errno::Badf))?
            .spelling
            .trailing(path);
        if let Found::Existing { node, .. } = &found
            && trailing
            && !self.is_directory(tree, *node).map_err(errno)?
        {
            return Err(errno(Errno::Notdir));
        }
        Ok(Resolved {
            tree,
            found,
            trailing,
        })
    }

    /// The tree a path given to the root directory reaches, and the names it walks from that tree's root: of every tree whose own spelling reads the path as one below its root, the one whose root is deepest, as wasi-libc gives a path to the preopen whose name is its longest prefix.
    fn rooted<'path>(&self, path: &'path str) -> Option<(usize, Vec<&'path str>)> {
        let mut deepest: Option<(usize, usize, Vec<&'path str>)> = None;
        for (index, tree) in self.trees.iter().enumerate() {
            let Some((depth, names)) = tree.spelling.reach(&tree.guest_path, path) else {
                continue;
            };
            if deepest.as_ref().is_none_or(|(held, _, _)| depth > *held) {
                deepest = Some((depth, index, names));
            }
        }
        deepest.map(|(_depth, index, names)| (index, names))
    }

    /// Where one more `name` of a path leads from `found`, `chain` the directories walked so far.
    fn step(
        &self,
        tree: usize,
        (found, chain): (&Found, &mut Vec<NodeId>),
        (name, last): (&str, bool),
    ) -> Result<Found, Fault> {
        let current = match found {
            Found::Existing { node, .. } => *node,
            Found::Absent { .. } => return Err(errno(Errno::Noent)),
        };
        let entries = self.entries(tree, current).map_err(errno)?;
        match name {
            "." => Ok(Found::Existing {
                node: current,
                place: None,
            }),
            ".." => {
                chain.pop();
                chain
                    .last()
                    .map(|up| Found::Existing {
                        node: *up,
                        place: None,
                    })
                    .ok_or(Fault::Refused(RefusalReason::Escape))
            }
            name => match entries.get(name) {
                Some(child) => {
                    if self.is_directory(tree, *child).map_err(errno)? {
                        chain.push(*child);
                    }
                    Ok(Found::Existing {
                        node: *child,
                        place: Some((current, name.to_owned())),
                    })
                }
                None if entries.keys().any(|held| one_name(held, name)) => {
                    Err(Fault::Refused(RefusalReason::CaseOnly))
                }
                None if last => Ok(Found::Absent {
                    parent: current,
                    name: name.to_owned(),
                }),
                None => Err(errno(Errno::Noent)),
            },
        }
    }

    /// Charges the overlay for one name made: its bytes and a share for the entry holding it.
    fn charge_name(&mut self, name: &str) -> Result<(), Fault> {
        let full = Fault::Refused(RefusalReason::OverlayFull);
        let cost = u64::try_from(name.len())
            .map_err(|_wide| full)?
            .checked_add(NAME_COST)
            .ok_or(full)?;
        let overlay = self.overlay.checked_add(cost).ok_or(full)?;
        if overlay > self.limit {
            return Err(full);
        }
        self.overlay = overlay;
        Ok(())
    }

    /// Makes a node holding `held` under the name `name` of the directory `parent`.
    fn make(
        &mut self,
        tree: usize,
        (parent, name): (NodeId, &str),
        held: Held,
    ) -> Result<NodeId, Fault> {
        self.charge_name(name)?;
        let parent_inode = self.live(tree, parent).map_err(errno)?.inode;
        let started = self.started;
        let arena = &mut self.trees.get_mut(tree).ok_or(errno(Errno::Badf))?.nodes;
        let node = arena.len();
        arena.push(Live {
            held,
            inode: inode_of(parent_inode, name),
            parent,
            accessed: started,
            modified: started,
        });
        self.entries_mut(tree, parent)
            .map_err(errno)?
            .insert(name.to_owned(), node);
        Ok(node)
    }

    /// Opens `path` from the directory `fd`, creating a file where asked, and answers the new descriptor.
    pub(crate) fn open(&mut self, fd: u32, path: &str, request: OpenRequest) -> Result<u32, Fault> {
        if request.oflags & !OFLAGS_ALL != 0 || request.fdflags & !FDFLAGS_ALL != 0 {
            return Err(errno(Errno::Inval));
        }
        let mut needed = RIGHTS_PATH_OPEN;
        if request.oflags & OFLAGS_CREAT != 0 {
            needed |= RIGHTS_PATH_CREATE_FILE;
        }
        if request.oflags & OFLAGS_TRUNC != 0 {
            needed |= RIGHTS_PATH_FILESTAT_SET_SIZE;
        }
        let resolved = self.resolve(fd, path, Needs::All(needed))?;
        let passed_on = self
            .descriptor(fd, Needs::Nothing)
            .map_err(errno)?
            .inheriting;
        let tree = resolved.tree;
        let wants_directory = request.oflags & OFLAGS_DIRECTORY != 0;
        let node = match resolved.found {
            Found::Existing { node, .. } => {
                if request.oflags & OFLAGS_CREAT != 0 && request.oflags & OFLAGS_EXCL != 0 {
                    return Err(errno(Errno::Exist));
                }
                if wants_directory && !self.is_directory(tree, node).map_err(errno)? {
                    return Err(errno(Errno::Notdir));
                }
                node
            }
            Found::Absent { parent, name } => {
                if request.oflags & OFLAGS_CREAT == 0 {
                    return Err(errno(Errno::Noent));
                }
                if wants_directory || resolved.trailing {
                    return Err(errno(Errno::Isdir));
                }
                self.make(
                    tree,
                    (parent, &name),
                    Held::File(Contents::Overlay(Vec::new())),
                )?
            }
        };
        let (object, mask) = if self.is_directory(tree, node).map_err(errno)? {
            if request.rights & RIGHTS_FD_WRITE != 0 || request.oflags & OFLAGS_TRUNC != 0 {
                return Err(errno(Errno::Isdir));
            }
            let object = Object::Directory {
                tree,
                node,
                preopened: false,
            };
            (object, RIGHTS_DIRECTORY)
        } else {
            if request.oflags & OFLAGS_TRUNC != 0 {
                self.resize((tree, node), 0)?;
            }
            let object = Object::File {
                tree,
                node,
                position: 0,
            };
            (object, RIGHTS_FILE)
        };
        self.install(Descriptor {
            object,
            flags: request.fdflags,
            rights: request.rights & mask & passed_on,
            inheriting: request.inheriting & RIGHTS_ALL & passed_on,
        })
        .map_err(errno)
    }

    /// Gives `descriptor` the lowest free number.
    fn install(&mut self, descriptor: Descriptor) -> Result<u32, Errno> {
        if self.descriptors.len() >= MOST_DESCRIPTORS {
            return Err(Errno::Mfile);
        }
        let mut number = 0_u32;
        while self.descriptors.contains_key(&number) {
            number = number.checked_add(1).ok_or(Errno::Mfile)?;
        }
        self.descriptors.insert(number, descriptor);
        Ok(number)
    }

    /// Makes a directory at `path` from the directory `fd`.
    pub(crate) fn create_directory(&mut self, fd: u32, path: &str) -> Result<(), Fault> {
        let resolved = self.resolve(fd, path, Needs::All(RIGHTS_PATH_CREATE_DIRECTORY))?;
        match resolved.found {
            Found::Existing { .. } => Err(errno(Errno::Exist)),
            Found::Absent { parent, name } => self
                .make(
                    resolved.tree,
                    (parent, &name),
                    Held::Directory(Arc::new(BTreeMap::new())),
                )
                .map(|_made| ()),
        }
    }

    /// Removes the empty directory at `path` from the directory `fd`.
    pub(crate) fn remove_directory(&mut self, fd: u32, path: &str) -> Result<(), Fault> {
        let resolved = self.resolve(fd, path, Needs::All(RIGHTS_PATH_REMOVE_DIRECTORY))?;
        let tree = resolved.tree;
        match resolved.found {
            Found::Absent { .. } => Err(errno(Errno::Noent)),
            Found::Existing { place: None, .. } => Err(errno(Errno::Inval)),
            Found::Existing {
                node,
                place: Some((parent, name)),
            } => {
                if !self.entries(tree, node).map_err(errno)?.is_empty() {
                    return Err(errno(Errno::Notempty));
                }
                self.entries_mut(tree, parent).map_err(errno)?.remove(&name);
                Ok(())
            }
        }
    }

    /// Removes the file at `path` from the directory `fd`.
    pub(crate) fn unlink_file(&mut self, fd: u32, path: &str) -> Result<(), Fault> {
        let resolved = self.resolve(fd, path, Needs::All(RIGHTS_PATH_UNLINK_FILE))?;
        let tree = resolved.tree;
        match resolved.found {
            Found::Absent { .. } => Err(errno(Errno::Noent)),
            Found::Existing { place: None, .. } => Err(errno(Errno::Isdir)),
            Found::Existing {
                node,
                place: Some((parent, name)),
            } => {
                if self.is_directory(tree, node).map_err(errno)? {
                    return Err(errno(Errno::Isdir));
                }
                self.entries_mut(tree, parent).map_err(errno)?.remove(&name);
                Ok(())
            }
        }
    }

    /// Whether `ancestor` is `node` or holds it, walking up from `node`.
    fn holds(&self, tree: usize, ancestor: NodeId, node: NodeId) -> Result<bool, Errno> {
        let mut at = node;
        let bound = match self.trees.get(tree) {
            Some(held) => held.nodes.len(),
            None => return Err(Errno::Badf),
        };
        for _step in 0..=bound {
            if at == ancestor {
                return Ok(true);
            }
            let up = self.live(tree, at)?.parent;
            if up == at {
                return Ok(false);
            }
            at = up;
        }
        Ok(false)
    }

    /// The file `names` walk to from the root of tree `tree`, where there is one.
    pub(crate) fn file_at(&self, tree: usize, names: &[String]) -> Option<NodeId> {
        let mut at = ROOT;
        for name in names {
            let Ok(entries) = self.entries(tree, at) else {
                return None;
            };
            at = *entries.get(name)?;
        }
        match self.live(tree, at) {
            Ok(Live {
                held: Held::File(_),
                ..
            }) => Some(at),
            Ok(Live {
                held: Held::Directory(_),
                ..
            })
            | Err(_) => None,
        }
    }

    /// Moves what `from` names to `to`, each resolved from its own directory descriptor, and answers the tree and node it moved, or nothing where `to` already was what `from` names.
    pub(crate) fn rename(
        &mut self,
        from: (u32, &str),
        to: (u32, &str),
    ) -> Result<Option<(usize, NodeId)>, Fault> {
        let source = self.resolve(from.0, from.1, Needs::All(RIGHTS_PATH_RENAME_SOURCE))?;
        let target = self.resolve(to.0, to.1, Needs::All(RIGHTS_PATH_RENAME_TARGET))?;
        if source.tree != target.tree {
            return Err(errno(Errno::Xdev));
        }
        let tree = source.tree;
        let (node, (source_parent, source_name)) = match source.found {
            Found::Absent { .. } => return Err(errno(Errno::Noent)),
            Found::Existing { place: None, .. } => return Err(errno(Errno::Inval)),
            Found::Existing {
                node,
                place: Some(place),
            } => (node, place),
        };
        let into = match &target.found {
            Found::Existing {
                place: Some((parent, _)),
                ..
            }
            | Found::Absent { parent, .. } => Some(*parent),
            Found::Existing { place: None, .. } => None,
        };
        if let Some(into) = into
            && self.is_directory(tree, node).map_err(errno)?
            && self.holds(tree, node, into).map_err(errno)?
        {
            return Err(errno(Errno::Inval));
        }
        let Some((parent, name)) = self.destination(tree, node, target)? else {
            return Ok(None);
        };
        self.entries_mut(tree, source_parent)
            .map_err(errno)?
            .remove(&source_name);
        self.entries_mut(tree, parent)
            .map_err(errno)?
            .insert(name, node);
        self.live_mut(tree, node).map_err(errno)?.parent = parent;
        Ok(Some((tree, node)))
    }

    /// Where a rename of `node` to `target` puts it, or nothing where `target` already is `node`.
    fn destination(
        &mut self,
        tree: usize,
        node: NodeId,
        target: Resolved,
    ) -> Result<Option<(NodeId, String)>, Fault> {
        let moving_directory = self.is_directory(tree, node).map_err(errno)?;
        match target.found {
            Found::Existing { place: None, .. } => Err(errno(Errno::Inval)),
            Found::Existing {
                node: existing,
                place: Some((parent, name)),
            } => {
                if existing == node {
                    return Ok(None);
                }
                match (
                    moving_directory,
                    self.is_directory(tree, existing).map_err(errno)?,
                ) {
                    (true, false) => Err(errno(Errno::Notdir)),
                    (false, true) => Err(errno(Errno::Isdir)),
                    (true, true) if !self.entries(tree, existing).map_err(errno)?.is_empty() => {
                        Err(errno(Errno::Notempty))
                    }
                    (true, true) | (false, false) => Ok(Some((parent, name))),
                }
            }
            Found::Absent { parent, name } => {
                if target.trailing && !moving_directory {
                    return Err(errno(Errno::Notdir));
                }
                self.charge_name(&name)?;
                Ok(Some((parent, name)))
            }
        }
    }

    /// The `filestat` of what `path` names from the directory `fd`.
    pub(crate) fn path_filestat(&self, fd: u32, path: &str) -> Result<Filestat, Fault> {
        let resolved = self.resolve(fd, path, Needs::All(RIGHTS_PATH_FILESTAT_GET))?;
        match resolved.found {
            Found::Absent { .. } => Err(errno(Errno::Noent)),
            Found::Existing { node, .. } => self.node_stat(resolved.tree, node).map_err(errno),
        }
    }

    /// Sets the times of what `path` names from the directory `fd`.
    pub(crate) fn path_set_times(
        &mut self,
        fd: u32,
        path: &str,
        request: TimesRequest,
    ) -> Result<(), Fault> {
        let resolved = self.resolve(fd, path, Needs::All(RIGHTS_PATH_FILESTAT_SET_TIMES))?;
        match resolved.found {
            Found::Absent { .. } => Err(errno(Errno::Noent)),
            Found::Existing { node, .. } => self.touch(resolved.tree, node, request).map_err(errno),
        }
    }

    /// What `path_readlink` answers: no path names a symbolic link, because the overlay makes none.
    pub(crate) fn readlink(&self, fd: u32, path: &str) -> Fault {
        match self.resolve(fd, path, Needs::All(RIGHTS_PATH_READLINK)) {
            Ok(Resolved {
                found: Found::Existing { .. },
                ..
            }) => errno(Errno::Inval),
            Ok(Resolved {
                found: Found::Absent { .. },
                ..
            }) => errno(Errno::Noent),
            Err(fault) => fault,
        }
    }

    /// Whether `fd` is ready the way a poll subscription asks, and how many bytes and which flags its event carries.
    pub(crate) fn ready(&self, fd: u32, readiness: Readiness) -> Result<(u64, u16), Errno> {
        let right = match readiness {
            Readiness::Read => RIGHTS_FD_READ,
            Readiness::Write => RIGHTS_FD_WRITE,
        };
        let needs = Needs::All(right | RIGHTS_POLL_FD_READWRITE);
        match (self.stream(fd, needs)?, readiness) {
            (Stream::Stdin, Readiness::Read) => Ok((0, EVENTRWFLAGS_HANGUP)),
            (Stream::File, Readiness::Read) => {
                let opened = self.file(fd, needs)?;
                let (tree, node) = opened.at;
                let len = len_of(self.contents(tree, node)?).map_err(|_full| Errno::Overflow)?;
                Ok((len.saturating_sub(opened.position), 0))
            }
            (Stream::Stdout | Stream::Stderr | Stream::File, Readiness::Write) => Ok((0, 0)),
            (Stream::Stdout | Stream::Stderr, Readiness::Read)
            | (Stream::Stdin, Readiness::Write) => Err(Errno::Badf),
        }
    }

    /// Every path whose final state differs from its snapshot, tree by tree, each in the order a sorted walk meets it.
    pub(crate) fn overlay(&self) -> Result<Vec<OverlayEntry>, Errno> {
        let mut entries = Vec::new();
        for (index, tree) in self.trees.iter().enumerate() {
            let walk = Walk {
                filesystem: self,
                tree: index,
                base: tree.base.nodes(),
                guest_path: &tree.guest_path,
            };
            walk.compare((ROOT, ROOT), "", &mut entries)?;
        }
        Ok(entries)
    }
}

/// A descriptor for a standard stream.
const fn standard(object: Object, rights: u64) -> Descriptor {
    Descriptor {
        object,
        flags: 0,
        rights,
        inheriting: 0,
    }
}

/// The length of `bytes` as a file size.
fn len_of(bytes: &[u8]) -> Result<u64, Fault> {
    u64::try_from(bytes.len()).map_err(|_wide| errno(Errno::Overflow))
}

/// Up to `len` bytes of `bytes` from `offset`, nothing past its end.
fn slice_at(bytes: &[u8], offset: u64, len: usize) -> &[u8] {
    let Ok(start) = usize::try_from(offset) else {
        return &[];
    };
    let rest = match bytes.get(start..) {
        Some(rest) => rest,
        None => return &[],
    };
    rest.split_at(len.min(rest.len())).0
}
