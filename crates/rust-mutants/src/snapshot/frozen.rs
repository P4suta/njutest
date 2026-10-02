// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Immutable complete source graphs retained under their explicit cache owner.

use std::io;
use std::path::{Path, PathBuf};

use super::{FrozenGraph, Layout, Options, Snapshot, State, Survey};

impl FrozenGraph {
    pub(crate) fn take(mut snapshot: Snapshot, rules: &Options) -> io::Result<Self> {
        let placement = rules.layout.under(snapshot.dir.join(super::TREE_NAME));
        let beside = placement
            .beside()
            .iter()
            .map(|one| one.destination().to_path_buf())
            .collect();
        let mut graph = Self {
            directory: snapshot.dir.clone(),
            root: snapshot.root.clone(),
            beside,
            manifest: snapshot.manifest.clone(),
            passed_over: snapshot.passed_over.clone(),
            inventory: Survey {
                rules: String::new(),
                files: std::collections::BTreeMap::new(),
                passed_over: std::collections::BTreeMap::new(),
            },
        };
        graph.inventory = super::survey(&graph.rules(PathBuf::new())?).map_err(io::Error::other)?;
        if let Some(owner) = &mut snapshot.owner {
            owner.release()?;
        }
        let mut owner = crate::tempowner::claim_cache(
            &snapshot.dir,
            jiff::Timestamp::now(),
            "rust-mutants-frozen-source-v1",
        )
        .map_err(io::Error::other)?;
        owner.release()?;
        snapshot.state = State::Released;
        Ok(graph)
    }

    pub(crate) fn rules(&self, parent: PathBuf) -> io::Result<Options> {
        let layout = Layout::plan(&self.root, &self.beside).map_err(io::Error::other)?;
        Ok(Options::new(layout, parent))
    }

    pub(crate) fn verify(&self, parent: &Path) -> io::Result<()> {
        if self.directory.parent() != Some(parent)
            || !self.root.starts_with(self.directory.join(super::TREE_NAME))
            || self
                .beside
                .iter()
                .any(|path| !path.starts_with(self.directory.join(super::TREE_NAME)))
        {
            return Err(io::Error::other(
                "an immutable graph is outside its retained owner",
            ));
        }
        let current = super::survey(&self.rules(PathBuf::new())?).map_err(io::Error::other)?;
        if current != self.inventory {
            return Err(io::Error::other("the immutable source graph changed"));
        }
        Ok(())
    }

    pub(crate) fn bind_copy(&self, snapshot: &mut Snapshot, original: &Path) -> io::Result<()> {
        if snapshot.manifest != self.manifest {
            return Err(io::Error::other(
                "the mutable copy differs from its immutable source graph",
            ));
        }
        snapshot.source_root = original.to_path_buf();
        snapshot.passed_over.clone_from(&self.passed_over);
        snapshot.workspace_digest = super::digest_of(&snapshot.manifest, &snapshot.passed_over)
            .map_err(io::Error::other)?;
        Ok(())
    }
}
