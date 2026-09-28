// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A repository's facts as a value: what git lists of it and what cargo reads of its graphs, read once and handed to every check that decides from them.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::GateError;
use crate::repository::ListingError;
use crate::sentinel::Laid;

/// The questions a tree's facts are read by, handed to the reading so a test can count or answer each one.
pub trait Ask {
    /// Every file git lists of the repository at `root`, as [`crate::repository::files`] reads it.
    ///
    /// # Errors
    /// What [`crate::repository::files`] refuses.
    fn listed(&self, root: &Path) -> Result<Vec<String>, ListingError>;

    /// What cargo reads of the graph whose manifest is `manifest`, to the depth `depth` names.
    ///
    /// # Errors
    /// Cargo could not be run, refused the graph, or answered with something that is not metadata.
    fn read(
        &self,
        manifest: &Path,
        depth: Depth,
    ) -> Result<cargo_metadata::Metadata, cargo_metadata::Error>;
}

/// How much of a graph cargo is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Every package its lock file resolves, with every feature on.
    Resolved,
    /// The packages of its workspace alone.
    Members,
}

/// The questions put to git and cargo themselves.
#[derive(Debug, Clone, Copy)]
pub struct Processes;

impl Ask for Processes {
    fn listed(&self, root: &Path) -> Result<Vec<String>, ListingError> {
        crate::repository::files(root)
    }

    fn read(
        &self,
        manifest: &Path,
        depth: Depth,
    ) -> Result<cargo_metadata::Metadata, cargo_metadata::Error> {
        let mut command = cargo_metadata::MetadataCommand::new();
        command.manifest_path(manifest);
        match depth {
            Depth::Resolved => command
                .features(cargo_metadata::CargoOpt::AllFeatures)
                .other_options(vec!["--locked".to_owned()])
                .exec(),
            Depth::Members => command.no_deps().exec(),
        }
    }
}

/// A repository's facts, read once: where it lies, every file git lists of it, and what cargo reads of its graphs.
#[derive(Debug, Clone)]
pub struct Tree {
    root: PathBuf,
    files: Vec<String>,
    graphs: Graphs,
}

impl Tree {
    /// Reads the repository at `root` with one listing, then cargo's reading of each graph the listing holds.
    ///
    /// # Errors
    /// The tree cannot be listed or holds no closed source set, a manifest names a path outside the source roots, or cargo cannot read a graph.
    pub fn read_with(root: &Path, ask: &impl Ask) -> Result<Self, GateError> {
        let files = ask.listed(root)?;
        super::validate_closed_source_inventory(&files)?;
        let graphs = Graphs::read(root, &files, ask)?;
        Ok(Self {
            root: root.to_path_buf(),
            files,
            graphs,
        })
    }

    /// The tree `laid` holds, which cargo reads as it read the skeleton unless a planted file changes what cargo reads.
    ///
    /// # Errors
    /// What [`Tree::read_with`] refuses, for the planted files, for the skeleton, and for cargo's reading where it is asked again.
    pub fn planted(laid: &Laid, skeleton: &Skeleton, ask: &impl Ask) -> Result<Self, GateError> {
        super::validate_closed_source_inventory(&laid.files)?;
        let skeleton = skeleton.graphs(ask)?;
        let graphs = if laid.planted.iter().all(|path| skeleton.keeps(path)) {
            skeleton.moved(&laid.root)?
        } else {
            Graphs::read(&laid.root, &laid.files, ask)?
        };
        Ok(Self {
            root: laid.root.clone(),
            files: laid.files.clone(),
            graphs,
        })
    }

    /// Where it lies, as it was named.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every file it holds, by its repository-relative slash path in byte order.
    #[must_use]
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// What cargo reads of its graphs.
    #[must_use]
    pub const fn graphs(&self) -> &Graphs {
        &self.graphs
    }
}

/// Cargo's reading of the skeleton every planted tree is laid over, taken the first time a tree needs it and shared by every tree after.
#[derive(Debug)]
pub struct Skeleton(OnceLock<Result<Graphs, GateError>>);

impl Skeleton {
    /// Nothing read yet.
    #[must_use]
    pub const fn unread() -> Self {
        Self(OnceLock::new())
    }

    /// Cargo's reading of the skeleton, asked through `ask` the first time and remembered, answer or refusal, after.
    ///
    /// # Errors
    /// The skeleton could not be laid, or cargo could not read it.
    pub fn graphs(&self, ask: &impl Ask) -> Result<&Graphs, GateError> {
        match self.0.get_or_init(|| {
            let root = tempfile::tempdir().map_err(|error| {
                GateError(format!(
                    "lints: a directory to lay the skeleton in: {error}"
                ))
            })?;
            let laid = crate::sentinel::lay(root.path(), &[])
                .map_err(|error| GateError(format!("lints: laying the skeleton: {error}")))?;
            Graphs::read(root.path(), &laid.files, ask)
        }) {
            Ok(graphs) => Ok(graphs),
            Err(error) => Err(error.clone()),
        }
    }
}

/// What cargo reads of a tree: the procedural macros each resolved graph builds, and every package a workspace of the tree holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graphs {
    read_at: PathBuf,
    resolved: Vec<Resolved>,
    members: Vec<Member>,
}

/// One graph a tree resolves: its name, its lock file, and the procedural macros cargo reports it building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the procedural-macro inventory calls it.
    pub graph: &'static str,
    /// Its lock file, by repository-relative path.
    pub lock: &'static str,
    /// Every procedural-macro package it builds, as `name version source`.
    pub proc_macros: BTreeSet<String>,
}

/// One package a workspace of the tree holds, as cargo read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its name.
    pub name: String,
    /// Its manifest, canonical.
    pub manifest: PathBuf,
    /// Its targets, in the order cargo lists them.
    pub targets: Vec<Target>,
}

/// One target of a package, as cargo read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Its name.
    pub name: String,
    /// The file it is compiled from, canonical.
    pub source: PathBuf,
    /// Whether it is a procedural macro.
    pub proc_macro: bool,
}

/// The graphs a tree resolves, each by its name, its manifest and its lock file.
const RESOLVED: [(&str, &str, &str); 2] = [
    ("root", "Cargo.toml", "Cargo.lock"),
    ("fuzz", "fuzz/Cargo.toml", "fuzz/Cargo.lock"),
];

impl Graphs {
    /// Asks cargo once about each graph the tree at `root` resolves, and once about each graph a path dependency outside them reaches, after refusing any manifest in `files` that would have cargo read outside the source roots.
    ///
    /// # Errors
    /// A manifest names a path outside the source roots, cargo cannot read a graph, a path dependency lies outside the source roots, or a path cargo names cannot be resolved.
    pub fn read(root: &Path, files: &[String], ask: &impl Ask) -> Result<Self, GateError> {
        let read_at = canonical(root)?;
        super::preflight_cargo_manifests(&read_at, files)?;
        let mut resolved = Vec::new();
        let mut readings = Vec::new();
        for (graph, manifest, lock) in RESOLVED {
            let manifest = read_at.join(manifest);
            let metadata = ask.read(&manifest, Depth::Resolved).map_err(|error| {
                GateError(format!(
                    "cargo metadata --locked for {}: {error}",
                    manifest.display()
                ))
            })?;
            resolved.push(Resolved {
                graph,
                lock,
                proc_macros: metadata
                    .packages
                    .iter()
                    .filter(|package| {
                        package
                            .targets
                            .iter()
                            .any(cargo_metadata::Target::is_proc_macro)
                    })
                    .map(super::proc_macro_dependency_key)
                    .collect(),
            });
            readings.push(metadata);
        }
        let members = reached(&read_at, &readings, ask)?;
        Ok(Self {
            read_at,
            resolved,
            members,
        })
    }

    /// Each graph the tree resolves, in the order the inventory lists them.
    #[must_use]
    pub fn resolved(&self) -> &[Resolved] {
        &self.resolved
    }

    /// Every package a workspace of the tree holds, once each, in the order cargo reached it.
    #[must_use]
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// Whether planting `path` leaves what cargo reads as it was: a file cargo already reads as a target's source, or a Rust module below a `src` directory that is no target's root.
    fn keeps(&self, path: &str) -> bool {
        let planted = Path::new(path);
        let read = self
            .members
            .iter()
            .flat_map(|member| &member.targets)
            .any(|target| {
                target
                    .source
                    .strip_prefix(&self.read_at)
                    .is_ok_and(|within| within == planted)
            });
        read || path.split_once("/src/").is_some_and(|(_package, module)| {
            crate::repository::extension_is(module, "rs")
                && module != "lib.rs"
                && module != "main.rs"
                && !module.starts_with("bin/")
        })
    }

    /// The same reading, with every path it names moved from where it was read to the tree at `root`.
    fn moved(&self, root: &Path) -> Result<Self, GateError> {
        let to = canonical(root)?;
        let relocated = |path: &Path| match path.strip_prefix(&self.read_at) {
            Ok(within) => to.join(within),
            Err(_outside) => path.to_path_buf(),
        };
        Ok(Self {
            read_at: to.clone(),
            resolved: self.resolved.clone(),
            members: self
                .members
                .iter()
                .map(|member| Member {
                    name: member.name.clone(),
                    manifest: relocated(&member.manifest),
                    targets: member
                        .targets
                        .iter()
                        .map(|target| Target {
                            name: target.name.clone(),
                            source: relocated(&target.source),
                            proc_macro: target.proc_macro,
                        })
                        .collect(),
                })
                .collect(),
        })
    }
}

/// Every package a workspace holds that the resolved `readings` name or a path dependency of theirs reaches, asking cargo once about each graph outside them.
///
/// # Errors
/// A path cargo names cannot be resolved, a path dependency lies outside the source roots, or cargo cannot read a graph it reaches.
fn reached(
    read_at: &Path,
    readings: &[cargo_metadata::Metadata],
    ask: &impl Ask,
) -> Result<Vec<Member>, GateError> {
    let mut requested = BTreeSet::new();
    for (_graph, manifest, _lock) in RESOLVED {
        requested.insert(canonical(&read_at.join(manifest))?);
    }
    let mut walk = Walk {
        read_at,
        seen: BTreeSet::new(),
        members: Vec::new(),
    };
    let mut pending = VecDeque::new();
    for metadata in readings {
        pending.extend(walk.newly(metadata)?);
    }
    while let Some(manifest) = pending.pop_front() {
        if !requested.insert(manifest.clone()) {
            continue;
        }
        let metadata = ask.read(&manifest, Depth::Members).map_err(|error| {
            GateError(format!(
                "cargo metadata for {}: {error}",
                manifest.display()
            ))
        })?;
        pending.extend(walk.newly(&metadata)?);
    }
    Ok(walk.members)
}

/// The packages found so far on the way through a tree's graphs.
struct Walk<'a> {
    read_at: &'a Path,
    seen: BTreeSet<PathBuf>,
    members: Vec<Member>,
}

impl Walk<'_> {
    /// Takes in every package of `metadata`'s workspace not yet found, and says which manifests their path dependencies name that no package found so far has.
    ///
    /// # Errors
    /// A path cargo names cannot be resolved, or a path dependency lies outside the source roots.
    fn newly(&mut self, metadata: &cargo_metadata::Metadata) -> Result<Vec<PathBuf>, GateError> {
        let mut found = Vec::new();
        for package in metadata.workspace_packages() {
            let manifest =
                std::fs::canonicalize(package.manifest_path.as_std_path()).map_err(|error| {
                    GateError(format!("{}: {error}", package.manifest_path.as_str()))
                })?;
            if self.seen.insert(manifest.clone()) {
                self.members.push(member(package, manifest)?);
                found.push(package);
            }
        }
        let mut next = Vec::new();
        for package in found {
            for dependency in &package.dependencies {
                let Some(path) = &dependency.path else {
                    continue;
                };
                let directory = std::fs::canonicalize(path.as_std_path()).map_err(|error| {
                    GateError(format!("local dependency {}: {error}", path.as_str()))
                })?;
                if !super::inside_source_roots(self.read_at, &directory) {
                    return Err(GateError(format!(
                        "lints: local dependency {} is outside the four scanned source roots",
                        path.as_str()
                    )));
                }
                let manifest = canonical(&directory.join("Cargo.toml"))?;
                if !self.seen.contains(&manifest) {
                    next.push(manifest);
                }
            }
        }
        Ok(next)
    }
}

/// `package` as cargo read it, its manifest already resolved to `manifest`.
///
/// # Errors
/// A target's source cannot be resolved.
fn member(package: &cargo_metadata::Package, manifest: PathBuf) -> Result<Member, GateError> {
    let mut targets = Vec::new();
    for target in &package.targets {
        let source = std::fs::canonicalize(target.src_path.as_std_path()).map_err(|error| {
            GateError(format!("{} target {}: {error}", package.name, target.name))
        })?;
        targets.push(Target {
            name: target.name.clone(),
            source,
            proc_macro: target.is_proc_macro(),
        });
    }
    Ok(Member {
        name: package.name.to_string(),
        manifest,
        targets,
    })
}

/// `path` resolved.
///
/// # Errors
/// It cannot be.
fn canonical(path: &Path) -> Result<PathBuf, GateError> {
    std::fs::canonicalize(path).map_err(|error| GateError(format!("{}: {error}", path.display())))
}
