// SPDX-FileCopyrightText: 2026 njutest contributors
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
use crate::trace::{DiscoverFileRecord, Recorder, SkipClaimRecord, SkipCount};

/// Configures [`discover`].
#[derive(Debug, Clone)]
pub struct DiscoverOptions<'r> {
    /// The rules to apply.
    pub selection: Selection<'r>,
    /// Patterns a file must match to be mutable, against its workspace-relative path.
    /// Empty includes everything.
    pub include: Vec<Pattern>,
    /// Patterns that remove a file again; an exclude always wins.
    pub exclude: Vec<Pattern>,
    /// The files a change set leaves to mutate among those `include` and `exclude` select, the rest carrying entry markers only.
    /// Empty narrows nothing.
    pub narrowing: Vec<Pattern>,
    /// The member packages to discover in, by name.
    /// Empty means every member.
    /// A package left out is not a skip: nothing was decided about it.
    pub packages: Vec<String>,
    /// The places a reviewer configured the run to pass over, each with the reason they gave.
    pub skips: Vec<SkipRule>,
}

/// One `[[mutation.skip]]` entry: where to pass over, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipRule {
    /// The paths it speaks about, as a glob against the workspace-relative path.
    pub path: Pattern,
    /// The lines it speaks about, inclusive and 1-based.
    /// `None` is every line of the file.
    pub lines: Option<(u32, u32)>,
    /// The item it speaks about, by a suffix of the item path.
    /// `None` is every item.
    pub item: Option<String>,
    /// Source text every line it hides holds, which follows the code where a line number does not.
    /// `None` is any text.
    pub text: Option<String>,
    /// Why its author wrote it.
    pub reason: String,
}

impl SkipRule {
    /// Whether this entry speaks about a place, whose line holds `held`.
    #[must_use]
    pub fn covers(&self, path: &str, (line, held): (u32, &str), item: &str) -> bool {
        if !self.path.matches(path) {
            return false;
        }
        if let Some(text) = &self.text
            && !held.contains(text.as_str())
        {
            return false;
        }
        if let Some((from, to)) = self.lines
            && !(from..=to).contains(&line)
        {
            return false;
        }
        self.item
            .as_ref()
            .is_none_or(|wanted| item == wanted || item.ends_with(&format!("::{wanted}")))
    }
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

/// Every place an entry's anchoring text is, by file and 1-based line.
pub type Anchored = Vec<(String, u32)>;

/// One `rust-mutants: skip` marker, where it sits and whether it hid anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipClaim {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// The 1-based line the marker sits on.
    pub line: u32,
    /// The reason its author wrote.
    pub reason: String,
    /// Whether a place a rule targets starts inside what it speaks about.
    pub matched: bool,
    /// The source text a configured entry anchors to, when it names one.
    pub text: Option<String>,
    /// Every place that text is, by file and 1-based line, in the files the entry names.
    pub text_at: Anchored,
}

/// One decision the walk took, and the file it took it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// Where the place is.
    pub position: crate::syntax::Position,
    /// The rule, or the reason's name for a place that is not a rule's.
    pub rule: String,
    /// The guard form of a candidate.
    pub form: Option<crate::syntax::Form>,
    /// The reason of a skip.
    pub skip: Option<SkipReason>,
    /// What the walk has to say about the decision beyond its reason.
    pub note: Option<String>,
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
    /// Every `rust-mutants: skip` marker of a file the run measures, in (path, line) order.
    /// A marker in a file the run passed over is a marker about nothing this run decided.
    pub claims: Vec<SkipClaim>,
    /// Every decision the walk took, in (path, offset) order, for a reader asking about one place rather than about a tally.
    pub decisions: Vec<Decided>,
    /// The catalog of the candidates.
    pub catalog: Catalog,
    /// Every file the configuration selects and the change set left out, by path: nothing in it is mutated, and it is instrumented for entry like every other, so what an execution entered does not depend on the change set.
    pub marked_only: BTreeSet<String>,
}

/// Why discovery failed.
/// Rendered as `<code>: discover: <what>`.
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
    /// A source path has no exact UTF-8 spelling for the durable catalog.
    PathNotUtf8 {
        /// The path which cannot be represented without changing bytes.
        path: PathBuf,
    },
    /// The catalog could not be built.
    Catalog(#[from] BuildError),
    /// A candidate was incoherent.
    Candidate(#[from] CandidateError),
    /// A manifest discovery has to read is there and could not be read.
    Manifest(#[from] crate::cargo::CargoError),
    /// A selected package is not a member.
    UnknownPackage {
        /// The name.
        name: String,
    },
    /// One file yielded more candidates than the durable skip counter can represent.
    CandidateCountTooLarge {
        /// The workspace-relative source path.
        path: String,
        /// The exact in-memory count that was refused.
        count: usize,
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
            Self::PathNotUtf8 { path } => {
                write!(f, "{} has no exact UTF-8 catalog path", path.display())
            }
            Self::Catalog(error) => write!(f, "{error}"),
            Self::Candidate(error) => write!(f, "{error}"),
            Self::Manifest(error) => write!(f, "{error}"),
            Self::UnknownPackage { name } => {
                write!(f, "package {name:?} is not a workspace member")
            }
            Self::CandidateCountTooLarge { path, count } => {
                write!(
                    f,
                    "{path} yielded {count} candidates, beyond the report counter"
                )
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
            Self::Parse(error) => error.code(),
            Self::OutsideRoot { .. } | Self::PathNotUtf8 { .. } => error::DISCOVER_OUTSIDE_ROOT,
            Self::Catalog(_) | Self::Candidate(_) | Self::CandidateCountTooLarge { .. } => {
                error::DISCOVER_CATALOG_FAILED
            }
            Self::UnknownPackage { .. } => error::DISCOVER_UNKNOWN_PACKAGE,
            Self::Manifest(error) => error.code(),
        }
    }
}

/// How a file is treated, in priority order when targets disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Forbidden,
    Mutable,
    NoStd,
    TestOnly,
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

/// Finds every candidate in the workspace, walking the files its units compiled.
/// The trace receives one `discover-file` event per file.
///
/// # Errors
/// See [`DiscoverError`].
pub fn discover(
    input: &Input<'_>,
    options: &DiscoverOptions<'_>,
    trace: &Recorder,
) -> Result<Discovery, DiscoverError> {
    let root = input.root;
    let assigner = assigned(input, options)?;
    let assignments = assigner.assignments;
    let mut files = Vec::new();
    let mut candidates = Vec::new();
    let mut skips = Vec::new();
    let mut claims: Vec<SkipClaim> = Vec::new();
    let mut decisions: Vec<Decided> = Vec::new();
    let mut marked_only = BTreeSet::new();
    let mut configured = vec![false; options.skips.len()];
    let mut anchored: Vec<Anchored> = vec![Vec::new(); options.skips.len()];
    let mut builder = Builder::new();
    for (path, package) in &assigner.generated {
        let report = whole_file(path, package, SkipReason::GeneratedOutsideWorkspace);
        skips.extend(report.skips.iter().cloned());
        files.push(report);
    }
    let read: Vec<(&String, Result<FileDiscovery, DiscoverError>)> = assignments
        .keys()
        .map(|path| (path, walk(root, path, &options.selection)))
        .collect();
    let fragments = pasted_in(&read);
    for ((path, discovery), assignment) in read.into_iter().zip(assignments.values()) {
        let mut discovery = match discovery {
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
        let role = skipped(path, assignment.role, options, &mut marked_only);
        if role.is_none() {
            configure(
                root,
                &mut discovery,
                &options.skips,
                (&mut configured, &mut anchored),
            )?;
        }
        let report = report(&discovery, &assignment.package, role)?;
        trace.discover_file(record(&discovery, &report)?);
        if role.is_none() {
            claimed(path, &discovery.annotations, trace, &mut claims);
            decisions.extend(discovery.decisions.iter().map(|one| Decided {
                path: path.clone(),
                position: one.position,
                rule: one.rule.clone(),
                form: one.form,
                skip: one.skip,
                note: one.note.clone(),
            }));
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
    configured_claims(&options.skips, (&configured, &anchored), trace, &mut claims);
    claims.sort_by(|one, other| (&one.path, one.line).cmp(&(&other.path, other.line)));
    let catalog = builder.build()?;
    Ok(Discovery {
        files,
        candidates,
        skips,
        claims,
        decisions,
        catalog,
        marked_only,
    })
}

/// Which unit compiled each file the run may mutate, and what each file is to the run.
fn assigned<'a>(
    input: &'a Input<'a>,
    options: &DiscoverOptions<'_>,
) -> Result<Assigner<'a>, DiscoverError> {
    let members = selected_members(input.metadata, &options.packages)?;
    let physical_root = match crate::canonical::canonical(input.root) {
        Ok(physical_root) => physical_root,
        Err(_root_has_no_physical_spelling) => input.root.to_path_buf(),
    };
    let mut assigner = Assigner {
        root: input.root,
        physical_root,
        units: input.units,
        workspace_manifest: input
            .metadata
            .workspace_root
            .join(crate::cargo::manifest::FILE_NAME),
        assignments: BTreeMap::new(),
        generated: BTreeMap::new(),
    };
    for package in &members {
        for target in &package.targets {
            assigner.assign(package, target)?;
        }
    }
    Ok(assigner)
}

/// The text of the file at `path` when an entry that names it anchors to text, and nothing otherwise, since only such an entry reads a line.
fn anchoring(root: &Path, path: &str, rules: &[SkipRule]) -> Result<String, DiscoverError> {
    if !rules
        .iter()
        .any(|rule| rule.text.is_some() && rule.path.matches(path))
    {
        return Ok(String::new());
    }
    std::fs::read_to_string(root.join(path)).map_err(|source| DiscoverError::Unreadable {
        path: path.to_owned(),
        source,
    })
}

/// Every line of the file at `path` that holds `rule`'s anchoring text, when the rule names that file.
fn anchored_in(rule: &SkipRule, path: &str, lines: &[&str]) -> Anchored {
    let Some(wanted) = &rule.text else {
        return Vec::new();
    };
    if !rule.path.matches(path) {
        return Vec::new();
    }
    (1_u32..)
        .zip(lines)
        .filter(|(_, line)| line.contains(wanted.as_str()))
        .map(|(number, _)| (path.to_owned(), number))
        .collect()
}

/// The configured entries, recorded and kept beside the markers.
fn configured_claims(
    rules: &[SkipRule],
    (matched, anchored): (&[bool], &[Anchored]),
    trace: &Recorder,
    into: &mut Vec<SkipClaim>,
) {
    for ((rule, matched), text_at) in rules.iter().zip(matched).zip(anchored) {
        let line = match rule.lines {
            Some((from, _to)) => from,
            None => 0,
        };
        let claim = SkipClaim {
            path: rule.path.to_string(),
            line,
            reason: rule.reason.clone(),
            matched: *matched,
            text: rule.text.clone(),
            text_at: text_at.clone(),
        };
        trace.skip_claim(SkipClaimRecord {
            path: claim.path.clone(),
            line: claim.line,
            reason: claim.reason.clone(),
            matched: claim.matched,
        });
        into.push(claim);
    }
}

/// Takes out of one file's walk what a `[[mutation.skip]]` entry speaks about, and notes every line of it that holds an entry's anchoring text.
fn configure(
    root: &Path,
    discovery: &mut FileDiscovery,
    rules: &[SkipRule],
    (matched, anchored): (&mut [bool], &mut [Anchored]),
) -> Result<(), DiscoverError> {
    if rules.is_empty() {
        return Ok(());
    }
    let text = anchoring(root, &discovery.path, rules)?;
    let lines: Vec<&str> = text.lines().collect();
    for (rule, at) in rules.iter().zip(anchored.iter_mut()) {
        at.extend(anchored_in(rule, &discovery.path, &lines));
    }
    let held = |line: u32| -> &str {
        let index = match usize::try_from(line) {
            Ok(line) => line.checked_sub(1),
            Err(_beyond_this_platform) => None,
        };
        match index.and_then(|index| lines.get(index)) {
            Some(held) => held,
            None => "",
        }
    };
    let candidate_count = discovery.candidates.len();
    let mut hidden = Some(0u32);
    let path = discovery.path.clone();
    discovery.candidates.retain(|found| {
        let Some((at, rule)) = rules.iter().enumerate().find(|(_at, rule)| {
            rule.covers(
                &path,
                (found.position.line, held(found.position.line)),
                &found.item,
            )
        }) else {
            return true;
        };
        if let Some(claimed) = matched.get_mut(at) {
            *claimed = true;
        }
        hidden = hidden.and_then(|count| count.checked_add(1));
        for decision in &mut discovery.decisions {
            if decision.offset == found.candidate.span.start
                && decision.rule == found.candidate.rule.name
            {
                decision.form = None;
                decision.skip = Some(SkipReason::Configured);
                decision.note = Some(rule.reason.clone());
            }
        }
        false
    });
    let hidden = hidden.ok_or_else(|| DiscoverError::CandidateCountTooLarge {
        path: path.clone(),
        count: candidate_count,
    })?;
    if hidden == 0 {
        return Ok(());
    }
    discovery.skips.push(Skip {
        reason: SkipReason::Configured,
        path,
        count: hidden,
    });
    discovery.skips.sort_by_key(|skip| skip.reason);
    Ok(())
}

/// The markers of one file the run measures, recorded and kept.
fn claimed(
    path: &str,
    annotations: &[crate::syntax::Claim],
    trace: &Recorder,
    into: &mut Vec<SkipClaim>,
) {
    for claim in annotations {
        trace.skip_claim(SkipClaimRecord {
            path: path.to_owned(),
            line: claim.line,
            reason: claim.reason.clone(),
            matched: claim.matched,
        });
        into.push(SkipClaim {
            path: path.to_owned(),
            line: claim.line,
            reason: claim.reason.clone(),
            matched: claim.matched,
            text: None,
            text_at: Vec::new(),
        });
    }
}

/// Every file another file pastes in where an expression goes.
fn pasted_in(read: &[(&String, Result<FileDiscovery, DiscoverError>)]) -> BTreeSet<String> {
    read.iter()
        .filter_map(|(_, discovery)| match discovery {
            Ok(discovery) => Some(discovery),
            Err(_) => None,
        })
        .flat_map(|discovery| discovery.includes.iter())
        .filter(|include| !include.at_item)
        .map(|include| include.path.clone())
        .collect()
}

/// The report of a file that is a fragment: no candidate, one skip, and the reason said out loud.
fn fragment(path: &str, package: &str) -> FileReport {
    whole_file(path, package, SkipReason::IncludedExpression)
}

/// One file passed over whole, for a reason the walk never had a chance to reach.
fn whole_file(path: &str, package: &str, reason: SkipReason) -> FileReport {
    FileReport {
        path: path.to_owned(),
        package: package.to_owned(),
        candidates: 0,
        skips: vec![Skip {
            path: path.to_owned(),
            reason,
            count: 1,
        }],
        whole_file: Some(reason),
    }
}

/// The name a report calls a file a build script wrote outside the tree.
fn generated_name(source: &Path) -> Result<String, DiscoverError> {
    let name = source
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| DiscoverError::PathNotUtf8 {
            path: source.to_path_buf(),
        })?;
    Ok(format!("{GENERATED_DIR}/{name}"))
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
    /// The filesystem's spelling of `root`, for platforms that report a compiler source through a physical alias of the snapshot path.
    physical_root: PathBuf,
    units: &'a [Unit],
    /// The workspace manifest, which a member's `[lints] workspace = true` inherits from.
    workspace_manifest: PathBuf,
    assignments: BTreeMap<String, Assignment>,
    /// Files a unit compiled from outside the tree, by the name a report calls them.
    generated: BTreeMap<String, String>,
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
        let crate_root = relative(self.root, &self.physical_root, &target.src_path)?;
        let mut own_root = false;
        for path in &non_test {
            match relative(self.root, &self.physical_root, path) {
                Ok(rel) if rel == crate_root => own_root = true,
                Ok(_) | Err(DiscoverError::OutsideRoot { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        let no_std =
            own_root && crate_root_is_freestanding(self.root, &crate_root, &target.edition)?;
        let forbidden = crate::cargo::manifest::forbidden(
            &package.manifest_path,
            Some(&self.workspace_manifest),
        )?;
        let forbids = crate_root_forbids_guard_noise(self.root, &crate_root, &forbidden)?;
        for unit in &compiled {
            for source in &unit.sources {
                let path = match relative(self.root, &self.physical_root, source) {
                    Ok(path) => path,
                    Err(DiscoverError::OutsideRoot { .. }) => {
                        self.generated
                            .insert(generated_name(source)?, package.name.clone());
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let role = if forbids {
                    Role::Forbidden
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

/// Whether the crate at `rel`, or the manifest that builds it, forbids a lint the guards fire.
fn crate_root_forbids_guard_noise(
    root: &Path,
    rel: &str,
    forbidden: &[String],
) -> Result<bool, DiscoverError> {
    let source =
        std::fs::read_to_string(root.join(rel)).map_err(|source| DiscoverError::Unreadable {
            path: rel.to_owned(),
            source,
        })?;
    Ok(forbids_guard_noise(&source, forbidden))
}

/// Whether a crate compiled this way forbids a lint the guards' own attribute turns off.
#[must_use]
pub fn forbids_guard_noise(source: &str, forbidden: &[String]) -> bool {
    if forbidden
        .iter()
        .any(|lint| crate::instrument::GENERATED_MODULE_CONFLICTING_LINTS.contains(&lint.as_str()))
    {
        return true;
    }
    let Ok(file) = syn::parse_file(source) else {
        return false;
    };
    file.attrs.iter().any(
        |attribute| match generated_module_forbidden_by(&attribute.meta) {
            Ok(forbidden) => forbidden,
            Err(_) => true,
        },
    )
}

/// Whether one crate attribute, including a conditional attribute, forbids a lint the generated support module must allow.
fn generated_module_forbidden_by(meta: &syn::Meta) -> Result<bool, syn::Error> {
    let syn::Meta::List(list) = meta else {
        return Ok(false);
    };
    if list.path.is_ident("forbid") {
        let nested = list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        return Ok(nested.iter().any(|entry| {
            let name = entry
                .path()
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            crate::instrument::GENERATED_MODULE_CONFLICTING_LINTS.contains(&name.as_str())
        }));
    }
    if !list.path.is_ident("cfg_attr") {
        return Ok(false);
    }
    let nested = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    )?;
    for attribute in nested.iter().skip(1) {
        if generated_module_forbidden_by(attribute)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the crate at `rel` is one this host cannot lend `std` to.
fn crate_root_is_freestanding(
    root: &Path,
    rel: &str,
    edition: &str,
) -> Result<bool, DiscoverError> {
    let text =
        std::fs::read_to_string(root.join(rel)).map_err(|source| DiscoverError::Unreadable {
            path: rel.to_owned(),
            source,
        })?;
    Ok(freestanding(&text, edition))
}

/// Whether a crate root's own text says the host cannot lend it `std`.
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

/// The directory a report puts a file a build script wrote outside the tree under.
pub const GENERATED_DIR: &str = "<generated>";

/// The workspace-relative, `/`-separated spelling of `path`.
fn relative(root: &Path, physical_root: &Path, path: &Path) -> Result<String, DiscoverError> {
    let outside = || DiscoverError::OutsideRoot {
        path: path.display().to_string(),
        root: root.display().to_string(),
    };
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel.to_path_buf(),
        Err(_not_under_logical_root) => {
            let physical_path = crate::canonical::canonical(path).map_err(|_error| outside())?;
            physical_path
                .strip_prefix(physical_root)
                .map(Path::to_path_buf)
                .map_err(|_error| outside())?
        }
    };
    let rel = rel
        .to_str()
        .ok_or_else(|| DiscoverError::PathNotUtf8 { path: rel.clone() })?;
    normalize_path(rel).map_err(|_error| outside())
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

/// Why a file is not mutated, or nothing where it is; a file only the change set left out is also noted as one to instrument for entry.
fn skipped(
    path: &str,
    role: Role,
    options: &DiscoverOptions<'_>,
    marked_only: &mut BTreeSet<String>,
) -> Option<SkipReason> {
    match role {
        Role::Forbidden => Some(SkipReason::ForbiddenLints),
        Role::Mutable if !selected_by_configuration(path, options) => Some(SkipReason::Excluded),
        Role::Mutable if !selected_by_change(path, options) => {
            marked_only.insert(path.to_owned());
            Some(SkipReason::Excluded)
        }
        Role::Mutable => None,
        Role::NoStd => Some(SkipReason::NoStdCrate),
        Role::TestOnly => Some(SkipReason::TestOnlyFile),
    }
}

fn selected_by_configuration(path: &str, options: &DiscoverOptions<'_>) -> bool {
    let included = options.include.is_empty() || options.include.iter().any(|p| p.matches(path));
    included && !options.exclude.iter().any(|p| p.matches(path))
}

fn selected_by_change(path: &str, options: &DiscoverOptions<'_>) -> bool {
    options.narrowing.is_empty() || options.narrowing.iter().any(|p| p.matches(path))
}

fn report(
    discovery: &FileDiscovery,
    package: &str,
    whole_file: Option<SkipReason>,
) -> Result<FileReport, DiscoverError> {
    let (candidates, skips) = match whole_file {
        None => (discovery.candidates.len(), discovery.skips.clone()),
        Some(reason) => {
            let count =
                u32::try_from(discovery.candidates.len()).map_err(|_outside_report_counter| {
                    DiscoverError::CandidateCountTooLarge {
                        path: discovery.path.clone(),
                        count: discovery.candidates.len(),
                    }
                })?;
            let hidden = vec![Skip {
                reason,
                path: discovery.path.clone(),
                count,
            }];
            (0, hidden)
        }
    };
    Ok(FileReport {
        path: discovery.path.clone(),
        package: package.to_owned(),
        candidates,
        skips,
        whole_file,
    })
}

/// The trace record: the walk's decisions for a mutable file, the one whole-file tally otherwise.
fn record(
    discovery: &FileDiscovery,
    report: &FileReport,
) -> Result<DiscoverFileRecord, DiscoverError> {
    Ok(match report.whole_file {
        None => {
            discovery
                .trace_record()
                .map_err(|overflow| DiscoverError::CandidateCountTooLarge {
                    path: overflow.path,
                    count: overflow.count,
                })?
        }
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
    })
}
