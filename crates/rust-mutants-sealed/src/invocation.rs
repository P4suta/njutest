// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one invocation is a function of, as one value, and the digest of it.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use crate::digest::{Encoder, SealedDigest};
use crate::error::{EnvironmentFault, PreopenFault, SealedError, WorkingFault};
use crate::snapshot::Snapshot;
use crate::spelling::Spelling;
use crate::transcript::{OverlayEntry, OverlayState};

/// The arguments a guest reads, the program name first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arguments(Vec<String>);

impl Arguments {
    /// The arguments, refusing one a C string cannot carry.
    ///
    /// # Errors
    /// [`SealedError::ArgumentHoldsNul`] for an argument holding a NUL byte.
    pub fn new(values: Vec<String>) -> Result<Self, SealedError> {
        match values.iter().position(|value| value.contains('\0')) {
            Some(index) => Err(SealedError::ArgumentHoldsNul { index }),
            None => Ok(Self(values)),
        }
    }

    /// The arguments, the program name first.
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// The environment a guest reads, in the order of its names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment(BTreeMap<String, String>);

impl Environment {
    /// The variables, refusing one a guest cannot be given.
    ///
    /// # Errors
    /// [`SealedError::EnvironmentVariable`] for an empty name, a name holding `=` or NUL, a value holding NUL, or a name given twice.
    pub fn new(variables: Vec<(String, String)>) -> Result<Self, SealedError> {
        let mut held = BTreeMap::new();
        for (name, value) in variables {
            let fault = if name.is_empty() {
                Some(EnvironmentFault::EmptyName)
            } else if name.contains('=') {
                Some(EnvironmentFault::NameHoldsEquals)
            } else if name.contains('\0') || value.contains('\0') {
                Some(EnvironmentFault::HoldsNul)
            } else if held.contains_key(&name) {
                Some(EnvironmentFault::Repeated)
            } else {
                None
            };
            if let Some(fault) = fault {
                return Err(SealedError::EnvironmentVariable { name, fault });
            }
            held.insert(name, value);
        }
        Ok(Self(held))
    }

    /// Every variable, in the order of its name.
    pub fn variables(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// The name the guest's root directory is preopened by.
pub(crate) const ROOT_NAME: &str = "/";

/// One directory a guest is given before it starts, as a descriptor numbered from 3 in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preopen {
    /// A read-only snapshot the guest reaches at `path`.
    Tree {
        /// The path the guest names the tree by: its absolute path as the guest's build spells one, POSIX (`/…`) or Windows (`X:\…`).
        path: String,
        /// What the tree holds.
        snapshot: Snapshot,
    },
    /// The guest's root directory, `/`: an absolute path into a tree, spelled as that tree's build spells one, reaches the tree, and every other is refused as an escape.
    Root {
        /// The directory the guest starts in, where it starts in one.
        start: Option<Start>,
    },
}

/// The directory a guest starts in: a directory of a tree preopened before the root, which the host enters through the guest's own `chdir` before `_start`, as cargo starts a test in its package's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The path of the tree it is in, as that tree's own preopen gives it.
    pub tree: String,
    /// The directory, as `/`-separated names below the tree's root, empty for the root itself.
    pub directory: String,
}

/// The linker arguments a guest is built with so that the host can start it in a directory: the C library's `chdir`, kept although nothing else calls it and exported, and its `malloc`, exported, which holds the path `chdir` is given.
pub const START_LINK_ARGS: [&str; 3] = ["--undefined=chdir", "--export=chdir", "--export=malloc"];

/// The directories a guest may reach, in the order their descriptors are numbered from 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preopens(Vec<Laid>);

/// A preopen as the host lays it out: what was given, and what the host reads of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Laid {
    /// A tree.
    Tree {
        /// The path the guest names it by.
        path: String,
        /// What it holds.
        snapshot: Snapshot,
        /// How a path into it is read, as its root is spelled.
        spelling: Spelling,
    },
    /// The root directory.
    Root {
        /// The directory the guest starts in, as given, and the path its `chdir` is given, spelled as the tree's build spells one.
        start: Option<(Start, String)>,
    },
}

impl Preopens {
    /// The preopens, refusing a guest path the guest cannot be given and a directory to start in no tree holds.
    ///
    /// # Errors
    /// [`SealedError::Preopen`] for an empty guest path, one holding NUL, or two wasi-libc reads as one place; [`SealedError::WorkingDirectory`] for a directory to start in that names no tree given before the root, or no directory of it.
    pub fn new(preopens: Vec<Preopen>) -> Result<Self, SealedError> {
        let mut laid: Vec<Laid> = Vec::with_capacity(preopens.len());
        for preopen in preopens {
            let path = match &preopen {
                Preopen::Tree { path, .. } => path.as_str(),
                Preopen::Root { .. } => ROOT_NAME,
            };
            let fault = if path.is_empty() {
                Some(PreopenFault::Empty)
            } else if path.contains('\0') {
                Some(PreopenFault::HoldsNul)
            } else if laid
                .iter()
                .any(|earlier| place(earlier.name()) == place(path))
            {
                Some(PreopenFault::Repeated)
            } else {
                None
            };
            if let Some(fault) = fault {
                return Err(SealedError::Preopen {
                    path: path.to_owned(),
                    fault,
                });
            }
            let next = match preopen {
                Preopen::Tree { path, snapshot } => {
                    let spelling = Spelling::of(&path);
                    Laid::Tree {
                        path,
                        snapshot,
                        spelling,
                    }
                }
                Preopen::Root { start: None } => Laid::Root { start: None },
                Preopen::Root { start: Some(start) } => match entered(&laid, &start) {
                    Ok(path) => Laid::Root {
                        start: Some((start, path)),
                    },
                    Err(fault) => {
                        return Err(SealedError::WorkingDirectory {
                            tree: start.tree,
                            directory: start.directory,
                            fault,
                        });
                    }
                },
            };
            laid.push(next);
        }
        Ok(Self(laid))
    }

    /// Every preopen, in descriptor order, as the host lays it out.
    pub(crate) fn laid(&self) -> &[Laid] {
        &self.0
    }

    /// These preopens with each tree as `overlay`, a transcript's or a part of one, left it: an entry changes the tree whose guest path is the longest that holds it, in the order the overlay lists them, so an invocation given them starts where the one that wrote the overlay stopped.
    ///
    /// # Errors
    /// [`SealedError::SnapshotPath`] for an entry below no tree, or one its tree's snapshot cannot take; [`SealedError::WorkingDirectory`] where the change took away the directory the guest starts in.
    pub fn after(&self, overlay: &[OverlayEntry]) -> Result<Self, SealedError> {
        let mut changes: Vec<Vec<(&str, &OverlayState)>> = vec![Vec::new(); self.0.len()];
        for entry in overlay {
            let held = self
                .0
                .iter()
                .enumerate()
                .filter_map(|(at, laid)| match laid {
                    Laid::Tree { path, .. } => {
                        below(path, &entry.path).map(|rest| (at, path, rest))
                    }
                    Laid::Root { .. } => None,
                })
                .max_by_key(|(_, path, _)| path.len());
            let Some((at, _, rest)) = held else {
                return Err(SealedError::SnapshotPath {
                    path: entry.path.clone(),
                    fault: crate::error::SnapshotFault::NoParent,
                });
            };
            if let Some(tree) = changes.get_mut(at) {
                tree.push((rest, &entry.state));
            }
        }
        let mut preopens = Vec::with_capacity(self.0.len());
        for (laid, changed) in self.0.iter().zip(changes) {
            preopens.push(match laid {
                Laid::Tree { path, snapshot, .. } => Preopen::Tree {
                    path: path.clone(),
                    snapshot: snapshot.after(changed)?,
                },
                Laid::Root { start, .. } => Preopen::Root {
                    start: start.as_ref().map(|(start, _)| start.clone()),
                },
            });
        }
        Self::new(preopens)
    }

    /// The path the guest's `chdir` is given before `_start`, where the root names a directory to start in.
    pub(crate) fn start(&self) -> Option<&str> {
        self.0.iter().find_map(|laid| match laid {
            Laid::Root {
                start: Some((_start, path)),
            } => Some(path.as_str()),
            Laid::Root { start: None } | Laid::Tree { .. } => None,
        })
    }
}

/// Where `path` is below the tree preopened at `root`: empty for the root itself, or the names below it, where it is in the tree at all.
fn below<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    let root = root.trim_end_matches('/');
    let rest = path.strip_prefix(root)?;
    if rest.is_empty() || rest == "/" {
        return Some("");
    }
    rest.strip_prefix('/')
}

impl Laid {
    /// The name the guest is given it by.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Tree { path, .. } => path,
            Self::Root { .. } => ROOT_NAME,
        }
    }
}

/// The path the guest's `chdir` is given for `start`, which names a directory of a tree `laid` holds: the tree's path and the directory's names, joined as the tree's build joins a path.
fn entered(laid: &[Laid], start: &Start) -> Result<String, WorkingFault> {
    let found = laid.iter().find_map(|earlier| match earlier {
        Laid::Tree {
            path,
            snapshot,
            spelling,
        } if *path == start.tree => Some((path, snapshot, spelling)),
        Laid::Tree { .. } | Laid::Root { .. } => None,
    });
    let Some((path, snapshot, spelling)) = found else {
        return Err(WorkingFault::NoTree);
    };
    let names = names_of(&start.directory).ok_or(WorkingFault::NotNames)?;
    snapshot
        .directory(&names)
        .ok_or(WorkingFault::NotADirectory)?;
    if names.is_empty() {
        return Ok(path.clone());
    }
    Ok(match spelling {
        Spelling::Posix => format!("{}/{}", path.trim_end_matches('/'), names.join("/")),
        Spelling::Windows { .. } => format!(
            "{}\\{}",
            path.trim_end_matches(['\\', '/']),
            names.join("\\")
        ),
    })
}

/// Where wasi-libc, which every Rust guest resolves a path through, puts a preopen named `path`: without its leading `/` and `./`, `.` alone as nothing, and without its trailing `/`.
fn place(path: &str) -> &str {
    let mut rest = path;
    loop {
        if let Some(after) = rest.strip_prefix('/') {
            rest = after;
        } else if let Some(after) = rest.strip_prefix("./") {
            rest = after;
        } else if rest == "." {
            rest = "";
        } else {
            return rest.trim_end_matches('/');
        }
    }
}

/// The names of a directory to start in given as `/`-separated names, none where one is empty, `.`, `..` or holds NUL.
fn names_of(directory: &str) -> Option<Vec<&str>> {
    if directory.is_empty() {
        return Some(Vec::new());
    }
    let names: Vec<&str> = directory.split('/').collect();
    names
        .iter()
        .all(|name| !name.is_empty() && *name != "." && *name != ".." && !name.contains('\0'))
        .then_some(names)
}

/// The resource ceilings of an invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most linear memory the guest may hold, in bytes.
    pub memory: u64,
    /// The most bytes of standard output kept; the rest are counted.
    pub stdout: u64,
    /// The most bytes of standard error kept; the rest are counted.
    pub stderr: u64,
    /// The most bytes the overlay may hold: every byte of every file written, and a share for every name made.
    pub overlay: u64,
}

/// How the guest's clocks read: from fixed origins, moved only by fuel spent and by waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockPolicy {
    /// What the realtime clock reads before the guest has spent anything, in nanoseconds since the Unix epoch.
    pub realtime_origin: u64,
    /// What the monotonic clock reads before the guest has spent anything, in nanoseconds.
    pub monotonic_origin: u64,
    /// How many nanoseconds each unit of fuel spent moves the clocks, never zero so a wait on time ends.
    pub nanos_per_fuel: NonZeroU64,
}

/// Everything one invocation of a sealed module is a function of, as one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The arguments the guest reads, the program name first.
    pub arguments: Arguments,
    /// The environment the guest reads.
    pub environment: Environment,
    /// The directories the guest may reach.
    pub preopens: Preopens,
    /// The seed of the guest's random bytes.
    pub seed: u64,
    /// The fuel the guest may spend before it is stopped.
    pub fuel: u64,
    /// The resource ceilings.
    pub limits: Limits,
    /// How the guest's clocks read.
    pub clock: ClockPolicy,
    /// The absolute guest path, inside a tree given before, at which a rename that puts a file there ends the guest as [`crate::SealedStop::Halted`], in that call and with nothing after it; nothing where no rename halts it.
    pub halt: Option<String>,
}

/// Where an invocation halts, as the host finds it: a tree, counting trees alone, and the names below its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Halt {
    /// The tree.
    pub(crate) tree: usize,
    /// The names from its root to the path.
    pub(crate) names: Vec<String>,
}

impl Invocation {
    /// Where this invocation halts, refusing a path no tree it is given holds a place for.
    ///
    /// # Errors
    /// [`SealedError::Halt`] for a path below no tree's guest path, or naming nothing below it, or naming `.`, `..`, an empty name or NUL.
    pub(crate) fn halting(&self) -> Result<Option<Halt>, SealedError> {
        let Some(path) = &self.halt else {
            return Ok(None);
        };
        let trees = self.preopens.0.iter().filter_map(|laid| match laid {
            Laid::Tree { path, .. } => Some(path.as_str()),
            Laid::Root { .. } => None,
        });
        for (tree, root) in trees.enumerate() {
            let Some(below) = path.strip_prefix(root.trim_end_matches('/')) else {
                continue;
            };
            let Some(below) = below.strip_prefix('/') else {
                continue;
            };
            if let Some(names) = names_of(below).filter(|names| !names.is_empty()) {
                return Ok(Some(Halt {
                    tree,
                    names: names.into_iter().map(ToOwned::to_owned).collect(),
                }));
            }
        }
        Err(SealedError::Halt { path: path.clone() })
    }

    /// The digest of this invocation of the module `module` under the configuration `configuration`.
    pub(crate) fn digest(
        &self,
        module: &SealedDigest,
        configuration: &SealedDigest,
    ) -> SealedDigest {
        let mut encoder = Encoder::new("rust-mutants-sealed/invocation/v3");
        encoder.digest(configuration).digest(module);
        encoder.count(self.arguments.0.len());
        for argument in &self.arguments.0 {
            encoder.text(argument);
        }
        encoder.count(self.environment.0.len());
        for (name, value) in &self.environment.0 {
            encoder.text(name).text(value);
        }
        encoder.count(self.preopens.0.len());
        for laid in &self.preopens.0 {
            match laid {
                Laid::Tree { path, snapshot, .. } => {
                    encoder.tag(b'T').text(path).digest(snapshot.digest());
                }
                Laid::Root { start: None } => {
                    encoder.tag(b'R').tag(b'N');
                }
                Laid::Root {
                    start: Some((start, _path)),
                } => {
                    encoder
                        .tag(b'R')
                        .tag(b'S')
                        .text(&start.tree)
                        .text(&start.directory);
                }
            }
        }
        encoder
            .number(self.seed)
            .number(self.fuel)
            .number(self.limits.memory)
            .number(self.limits.stdout)
            .number(self.limits.stderr)
            .number(self.limits.overlay)
            .number(self.clock.realtime_origin)
            .number(self.clock.monotonic_origin)
            .number(self.clock.nanos_per_fuel.get());
        match &self.halt {
            Some(path) => encoder.tag(b'H').text(path),
            None => encoder.tag(b'N'),
        };
        encoder.finish()
    }
}
