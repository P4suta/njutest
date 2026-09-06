// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Discovery over a whole workspace: which files are mutable, which are passed over as a whole and why, and the catalog that results.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cargo::{Metadata, Package, Target, Unit};
use crate::catalog::{BuildError, Builder, CandidateError, Catalog};
use crate::error::{self, ErrorCode};
use crate::glob::Pattern;
use crate::id::normalize_path;
use crate::syntax::{
    FileDiscovery, Found, Selection, Skip, SkipReason, SyntaxError, discover_file,
};
use crate::trace::{DiscoverFileRecord, Recorder, SkipCount};

/// Configures [`discover`].
#[derive(Debug, Clone)]
pub struct DiscoverOptions<'r> {
    /// The rules to apply.
    pub selection: Selection<'r>,
    /// Patterns a file must match to be mutable, against its workspace-relative path. Empty includes everything.
    pub include: Vec<Pattern>,
    /// Patterns that remove a file again; an exclude always wins.
    pub exclude: Vec<Pattern>,
    /// The member packages to discover in, by name. Empty means every member. A package left out is not a skip: nothing was decided about it.
    pub packages: Vec<String>,
}

/// One candidate plus the package it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The candidate and its site.
    pub found: Found,
    /// The name of the package whose unit compiled the file.
    pub package: String,
}

/// What discovery decided about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// The package whose unit compiled it.
    pub package: String,
    /// Candidates the file yielded; zero for a file skipped as a whole.
    pub candidates: usize,
    /// The skips: the walk's own for a mutable file, or the one whole-file reason with the count of candidates it hid.
    pub skips: Vec<Skip>,
    /// The whole-file reason, when there is one.
    pub whole_file: Option<SkipReason>,
}

/// Everything discovery found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// One report per file, in path order.
    pub files: Vec<FileReport>,
    /// Every candidate, in (path, edit start, rule position) order.
    pub candidates: Vec<Located>,
    /// Every skip, in (reason, path) order.
    pub skips: Vec<Skip>,
    /// The catalog of the candidates.
    pub catalog: Catalog,
}

/// Why discovery failed. Rendered as `<code>: discover: <what>`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoverError {
    /// A source file could not be read.
    Unreadable {
        /// The workspace-relative path.
        path: String,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// A source file does not parse for the engine.
    Parse(#[from] SyntaxError),
    /// A unit compiled a file outside the root.
    OutsideRoot {
        /// The absolute path.
        path: String,
        /// The root.
        root: String,
    },
    /// The catalog could not be built.
    Catalog(#[from] BuildError),
    /// A candidate was incoherent.
    Candidate(#[from] CandidateError),
    /// A selected package is not a member.
    UnknownPackage {
        /// The name.
        name: String,
    },
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: discover: ", self.code().code)?;
        match self {
            Self::Unreadable { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Parse(error) => write!(f, "{error}"),
            Self::OutsideRoot { path, root } => {
                write!(f, "{path} lies outside the workspace root {root}")
            }
            Self::Catalog(error) => write!(f, "{error}"),
            Self::Candidate(error) => write!(f, "{error}"),
            Self::UnknownPackage { name } => {
                write!(f, "package {name:?} is not a workspace member")
            }
        }
    }
}

impl DiscoverError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unreadable { .. } => error::DISCOVER_FILE_UNREADABLE,
            Self::Parse(_) => error::DISCOVER_PARSE_FAILED,
            Self::OutsideRoot { .. } => error::DISCOVER_OUTSIDE_ROOT,
            Self::Catalog(_) | Self::Candidate(_) => error::DISCOVER_CATALOG_FAILED,
            Self::UnknownPackage { .. } => error::DISCOVER_UNKNOWN_PACKAGE,
        }
    }
}

/// How a file is treated, in priority order when targets disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Mutable,
    NoStd,
    TestOnly,
    ProcMacro,
}

/// One file the units named, with the package and the role it was given.
#[derive(Debug)]
struct Assignment {
    package: String,
    role: Role,
}

/// The workspace discovery reads.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The workspace root the units' sources are under: the snapshot root.
    pub root: &'a Path,
    /// The members and their targets.
    pub metadata: &'a Metadata,
    /// The units of a `cargo check --all-targets` of that root.
    pub units: &'a [Unit],
}

/// Finds every candidate in the workspace, walking the files its units compiled. The trace receives one `discover-file` event per file.
///
/// # Errors
/// See [`DiscoverError`].
pub fn discover(
    input: &Input<'_>,
    options: &DiscoverOptions<'_>,
    trace: &Recorder,
) -> Result<Discovery, DiscoverError> {
    let root = input.root;
    let members = selected_members(input.metadata, &options.packages)?;
    let mut assigner = Assigner {
        root,
        units: input.units,
        assignments: BTreeMap::new(),
    };
    for package in &members {
        for target in &package.targets {
            assigner.assign(package, target)?;
        }
    }
    let assignments = assigner.assignments;
    let mut files = Vec::new();
    let mut candidates = Vec::new();
    let mut skips = Vec::new();
    let mut builder = Builder::new();
    let read: Vec<(&String, Result<FileDiscovery, DiscoverError>)> = assignments
        .keys()
        .map(|path| (path, walk(root, path, &options.selection)))
        .collect();
    let fragments = pasted_in(&read);
    for ((path, discovery), assignment) in read.into_iter().zip(assignments.values()) {
        let discovery = match discovery {
            Ok(discovery) => discovery,
            Err(error) if fragments.contains(path.as_str()) => {
                let report = fragment(path, &assignment.package);
                skips.extend(report.skips.iter().cloned());
                files.push(report);
                drop(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let role = match assignment.role {
            Role::Mutable if !selected_by_patterns(path, options) => Some(SkipReason::Excluded),
            Role::Mutable => None,
            Role::NoStd => Some(SkipReason::NoStdCrate),
            Role::TestOnly => Some(SkipReason::TestOnlyFile),
            Role::ProcMacro => Some(SkipReason::ProcMacroCrate),
        };
        let report = report(&discovery, &assignment.package, role);
        trace.discover_file(record(&discovery, &report));
        if role.is_none() {
            for found in discovery.candidates {
                builder.add(found.candidate.clone())?;
                candidates.push(Located {
                    found,
                    package: assignment.package.clone(),
                });
            }
        }
        skips.extend(report.skips.iter().cloned());
        files.push(report);
    }
    skips.sort();
    let catalog = builder.build()?;
    Ok(Discovery {
        files,
        candidates,
        skips,
        catalog,
    })
}

/// Every file another file pastes in where an expression goes.
///
/// Such a file is a fragment of one program rather than a program, so nothing
/// parses it on its own and nothing can append a runtime module to it. Reading
/// its unparsability as a defect in the tree would refuse to measure a project
/// the compiler is perfectly happy with.
fn pasted_in(read: &[(&String, Result<FileDiscovery, DiscoverError>)]) -> BTreeSet<String> {
    read.iter()
        .filter_map(|(_, discovery)| discovery.as_ref().ok())
        .flat_map(|discovery| discovery.includes.iter())
        .filter(|include| !include.at_item)
        .map(|include| include.path.clone())
        .collect()
}

/// The report of a file that is a fragment: no candidate, one skip, and the reason said out loud.
fn fragment(path: &str, package: &str) -> FileReport {
    FileReport {
        path: path.to_owned(),
        package: package.to_owned(),
        candidates: 0,
        skips: vec![Skip {
            path: path.to_owned(),
            reason: SkipReason::IncludedExpression,
            count: 1,
        }],
        whole_file: Some(SkipReason::IncludedExpression),
    }
}

/// The members to discover in: every member, or the named ones.
fn selected_members<'m>(
    metadata: &'m Metadata,
    packages: &[String],
) -> Result<Vec<&'m Package>, DiscoverError> {
    let members: Vec<&Package> = metadata.members().collect();
    if packages.is_empty() {
        return Ok(members);
    }
    packages
        .iter()
        .map(|name| {
            members
                .iter()
                .copied()
                .find(|package| &package.name == name)
                .ok_or_else(|| DiscoverError::UnknownPackage { name: name.clone() })
        })
        .collect()
}

/// Gives every file of every target its role, from the units that compiled the target, keeping the higher-priority role when targets disagree.
struct Assigner<'a> {
    root: &'a Path,
    units: &'a [Unit],
    assignments: BTreeMap<String, Assignment>,
}

impl Assigner<'_> {
    fn assign(&mut self, package: &Package, target: &Target) -> Result<(), DiscoverError> {
        let structural = target.is_custom_build()
            || target.is_test()
            || target.is_bench()
            || target.is_example();
        if structural {
            return Ok(());
        }
        let compiled: Vec<&Unit> = self
            .units
            .iter()
            .filter(|unit| unit.package_id == package.id && unit.target.name == target.name)
            .collect();
        let non_test: Vec<&Path> = compiled
            .iter()
            .filter(|unit| !unit.test)
            .flat_map(|unit| unit.sources.iter().map(PathBuf::as_path))
            .collect();
        let crate_root = relative(self.root, &target.src_path)?;
        let no_std = non_test
            .iter()
            .any(|path| relative(self.root, path).is_ok_and(|rel| rel == crate_root))
            && crate_root_is_freestanding(self.root, &crate_root, &target.edition);
        for unit in &compiled {
            for source in &unit.sources {
                let path = relative(self.root, source)?;
                let role = if target.is_proc_macro() {
                    Role::ProcMacro
                } else if no_std {
                    Role::NoStd
                } else if unit.test && !non_test.contains(&source.as_path()) {
                    Role::TestOnly
                } else {
                    Role::Mutable
                };
                self.record(path, &package.name, role);
            }
        }
        Ok(())
    }

    fn record(&mut self, path: String, package: &str, role: Role) {
        match self.assignments.get_mut(&path) {
            Some(existing) if existing.role <= role => {}
            Some(existing) => {
                existing.role = role;
                package.clone_into(&mut existing.package);
            }
            None => {
                self.assignments.insert(
                    path,
                    Assignment {
                        package: package.to_owned(),
                        role,
                    },
                );
            }
        }
    }
}

/// Whether the crate at `rel` is one this host cannot lend `std` to. A root that does not parse is answered `false` here; the walk reports the parse failure.
///
/// `#![no_std]` on its own is not that crate. It withholds the implicit link
/// to `std` and its prelude, and forbids neither an explicit link nor an
/// explicit path, so the runtime module borrows `std` under a name of its own
/// and the crate is measured like any other. What cannot be measured is a
/// crate that would then have two of something only one of which may exist: a
/// `#[panic_handler]` or a `#[global_allocator]` of its own, which `std`
/// brings too, or a `#![no_main]` crate, whose entry point `std` also
/// supplies. Edition 2015 is left out because `extern crate` resolves
/// differently there and the engine does not test what it does not run.
fn crate_root_is_freestanding(root: &Path, rel: &str, edition: &str) -> bool {
    std::fs::read_to_string(root.join(rel)).is_ok_and(|text| freestanding(&text, edition))
}

/// Whether a crate root's own text says the host cannot lend it `std`.
///
/// A root that does not parse is answered `false` here; the walk reports the
/// parse failure. `#![cfg_attr(not(test), no_std)]` is not `#![no_std]`: under
/// `cfg(test)` — which is how every test of the crate is built — the crate has
/// `std`, and this answers about the crate as its tests will see it.
#[must_use]
pub fn freestanding(source: &str, edition: &str) -> bool {
    let Ok(file) = syn::parse_file(source) else {
        return false;
    };
    if !file.attrs.iter().any(|attr| attr.path().is_ident("no_std")) {
        return false;
    }
    if edition == "2015" {
        return true;
    }
    file.attrs
        .iter()
        .any(|attr| attr.path().is_ident("no_main"))
        || file.items.iter().any(item_supplies_what_std_does)
}

/// Whether the item is one `std` also supplies, so that linking `std` beside it would be two of something only one of which may exist.
fn item_supplies_what_std_does(item: &syn::Item) -> bool {
    let attrs = match item {
        syn::Item::Fn(one) => &one.attrs,
        syn::Item::Static(one) => &one.attrs,
        _ => return false,
    };
    attrs.iter().any(|attr| {
        attr.path().is_ident("panic_handler") || attr.path().is_ident("global_allocator")
    })
}

/// The workspace-relative, `/`-separated spelling of `path`.
fn relative(root: &Path, path: &Path) -> Result<String, DiscoverError> {
    let outside = || DiscoverError::OutsideRoot {
        path: path.display().to_string(),
        root: root.display().to_string(),
    };
    let rel = path.strip_prefix(root).map_err(|_error| outside())?;
    normalize_path(&rel.to_string_lossy()).map_err(|_error| outside())
}

fn walk(
    root: &Path,
    path: &str,
    selection: &Selection<'_>,
) -> Result<FileDiscovery, DiscoverError> {
    let bytes = std::fs::read(root.join(path)).map_err(|source| DiscoverError::Unreadable {
        path: path.to_owned(),
        source,
    })?;
    Ok(discover_file(path, &bytes, selection)?)
}

fn selected_by_patterns(path: &str, options: &DiscoverOptions<'_>) -> bool {
    let included = options.include.is_empty() || options.include.iter().any(|p| p.matches(path));
    included && !options.exclude.iter().any(|p| p.matches(path))
}

fn report(discovery: &FileDiscovery, package: &str, whole_file: Option<SkipReason>) -> FileReport {
    let (candidates, skips) = whole_file.map_or_else(
        || (discovery.candidates.len(), discovery.skips.clone()),
        |reason| {
            let hidden = vec![Skip {
                reason,
                path: discovery.path.clone(),
                count: u32::try_from(discovery.candidates.len()).unwrap_or(u32::MAX),
            }];
            (0, hidden)
        },
    );
    FileReport {
        path: discovery.path.clone(),
        package: package.to_owned(),
        candidates,
        skips,
        whole_file,
    }
}

/// The trace record: the walk's decisions for a mutable file, the one whole-file tally otherwise.
fn record(discovery: &FileDiscovery, report: &FileReport) -> DiscoverFileRecord {
    match report.whole_file {
        None => discovery.trace_record(),
        Some(_) => DiscoverFileRecord {
            path: report.path.clone(),
            candidates: 0,
            sites: Vec::new(),
            skips: report
                .skips
                .iter()
                .map(|skip| SkipCount {
                    reason: skip.reason.name().to_owned(),
                    count: skip.count,
                })
                .collect(),
        },
    }
}
