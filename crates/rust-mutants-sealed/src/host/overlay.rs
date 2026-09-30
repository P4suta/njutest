// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The overlay's final contents: every path of a preopened tree whose state at the end differs from its snapshot.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::abi::Errno;
use crate::snapshot::{Body, Node, NodeId};
use crate::transcript::{OverlayEntry, OverlayState};

use super::fs::{Contents, Filesystem, Held, Live};

/// One tree compared with the snapshot it grew from.
pub(crate) struct Walk<'walk> {
    /// The filesystem holding the tree.
    pub(crate) filesystem: &'walk Filesystem,
    /// The tree.
    pub(crate) tree: usize,
    /// The snapshot's nodes.
    pub(crate) base: &'walk [Node],
    /// The guest path the tree is preopened at.
    pub(crate) guest_path: &'walk str,
}

impl Walk<'_> {
    /// The guest path of `relative`.
    fn guest(&self, relative: &str) -> String {
        if relative.is_empty() {
            self.guest_path.to_owned()
        } else if self.guest_path.ends_with('/') {
            format!("{}{relative}", self.guest_path)
        } else {
            format!("{}/{relative}", self.guest_path)
        }
    }

    /// Whether the live node `held` carries other times than the snapshot's node `base`, which a node the snapshot does not hold or holds without times is dated at by the instance.
    fn retimed(&self, base: NodeId, held: &Live) -> bool {
        let times = match self.base.get(base) {
            Some(node) => node.times,
            None => None,
        };
        match times {
            Some(times) => held.accessed != times.accessed || held.modified != times.modified,
            None => {
                let started = self.filesystem.started();
                held.accessed != started || held.modified != started
            }
        }
    }

    /// The entries of the snapshot's directory `node`.
    fn base_entries(&self, node: NodeId) -> Option<&BTreeMap<String, NodeId>> {
        match &self.base.get(node)?.body {
            Body::Directory(entries) => Some(entries),
            Body::File(_) => None,
        }
    }

    /// Adds every difference under the snapshot's directory `base` and the live directory `live`, both at `relative`.
    pub(crate) fn compare(
        &self,
        (base, live): (NodeId, NodeId),
        relative: &str,
        out: &mut Vec<OverlayEntry>,
    ) -> Result<(), Errno> {
        let held = self.filesystem.live(self.tree, live)?;
        if self.retimed(base, held) {
            out.push(OverlayEntry {
                path: self.guest(relative),
                state: OverlayState::Directory {
                    accessed: held.accessed,
                    modified: held.modified,
                },
            });
        }
        let nothing = BTreeMap::new();
        let before = match self.base_entries(base) {
            Some(entries) => entries,
            None => &nothing,
        };
        let after = self.filesystem.entries(self.tree, live)?;
        let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            match (before.get(name), after.get(name)) {
                (Some(_gone), None) => out.push(OverlayEntry {
                    path: self.guest(&path),
                    state: OverlayState::Removed,
                }),
                (None, Some(made)) => self.added(*made, &path, out)?,
                (Some(was), Some(is)) => self.changed((*was, *is), &path, out)?,
                (None, None) => {}
            }
        }
        Ok(())
    }

    /// Adds the live node `live` and everything under it, none of which the snapshot holds.
    fn added(
        &self,
        live: NodeId,
        relative: &str,
        out: &mut Vec<OverlayEntry>,
    ) -> Result<(), Errno> {
        let held = self.filesystem.live(self.tree, live)?;
        match &held.held {
            Held::File(contents) => {
                out.push(OverlayEntry {
                    path: self.guest(relative),
                    state: OverlayState::File {
                        contents: contents.bytes().to_vec(),
                        accessed: held.accessed,
                        modified: held.modified,
                    },
                });
                Ok(())
            }
            Held::Directory(entries) => {
                out.push(OverlayEntry {
                    path: self.guest(relative),
                    state: OverlayState::Directory {
                        accessed: held.accessed,
                        modified: held.modified,
                    },
                });
                for (name, child) in entries.iter() {
                    self.added(*child, &format!("{relative}/{name}"), out)?;
                }
                Ok(())
            }
        }
    }

    /// Adds the differences between the snapshot's node `base` and the live node `live`, both at `relative`.
    fn changed(
        &self,
        (base, live): (NodeId, NodeId),
        relative: &str,
        out: &mut Vec<OverlayEntry>,
    ) -> Result<(), Errno> {
        let held = self.filesystem.live(self.tree, live)?;
        let before = self.base.get(base).map(|node| &node.body);
        match (before, &held.held) {
            (Some(Body::File(was)), Held::File(is)) => {
                let same_bytes = match is {
                    Contents::Snapshot(is) => Arc::ptr_eq(was, is) || was == is,
                    Contents::Overlay(is) => **was == **is,
                };
                if !same_bytes || self.retimed(base, held) {
                    out.push(OverlayEntry {
                        path: self.guest(relative),
                        state: OverlayState::File {
                            contents: is.bytes().to_vec(),
                            accessed: held.accessed,
                            modified: held.modified,
                        },
                    });
                }
                Ok(())
            }
            (Some(Body::Directory(_)), Held::Directory(_)) => {
                self.compare((base, live), relative, out)
            }
            (
                Some(Body::File(_) | Body::Directory(_)) | None,
                Held::File(_) | Held::Directory(_),
            ) => self.added(live, relative, out),
        }
    }
}
