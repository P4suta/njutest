// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The probe tree: a build of the pristine program that records, for each test, which mutations would have changed something it computed.
//!
//! It is written over the same snapshot the instrumented tree later occupies,
//! built into a target directory of its own, and run once per test. Every site
//! the compiler refuses is left unprobed, and every test whose log this release
//! cannot read tells the run nothing about that test — both are the fail-closed
//! direction, because a missing infection is exactly the licence to skip work.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::discover::Discovery;
use crate::execute::{self, Context, ExecRequest};
use crate::probe::instrument::{Site, rewrite};
use crate::probe::runtime::MODULE_STEM;
use crate::probe::{log, runtime};
use crate::runner::Cancel;
use crate::session::PrepareOptions;
use crate::span::Span;
use crate::splice::{Splice, apply, count_lines};
use crate::trace::Recorder;
use crate::workspace::{SessionError, Workspace};

/// What the probe pass established.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probed {
    /// Every mutant the compiler accepted a probe of.
    pub asked: BTreeSet<u32>,
    /// What each target infected, by target identity. A target with no entry told the run nothing.
    pub infected: BTreeMap<String, BTreeSet<u32>>,
    /// What this pass could not establish, by name, for a report's limitations.
    pub limitations: Vec<String>,
}

/// The limitation a run states when the probe tree would not build at all.
pub use crate::limitation::PROBE_TREE_NOT_BUILT as UNBUILDABLE;

/// The limitation a run states when a test's infection log could not be read.
pub use crate::limitation::PROBE_LOG_UNREADABLE as UNREADABLE_LOG;

/// What the pass is about.
#[derive(Debug, Clone, Copy)]
pub struct Asking<'a> {
    /// The snapshot the probe tree is written into and taken out of.
    pub workspace: &'a Workspace,
    /// What discovery found.
    pub discovery: &'a Discovery,
    /// The pristine bytes of every file that yielded a candidate.
    pub sources: &'a BTreeMap<String, Vec<u8>>,
    /// How the build and the runs are bounded.
    pub options: &'a PrepareOptions,
}

/// One file's probed text, and where each site landed in it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbedFile {
    path: String,
    text: String,
    placed: Vec<(Span, u32)>,
}

/// Runs the probe pass and reports what it established.
///
/// # Errors
/// Only a failure to write the tree or to put it back, which leaves the
/// snapshot in a state no later phase could trust. A probe tree that will not
/// build, or a log that cannot be read, is a limitation rather than an error:
/// the run then executes the mutants it would have executed anyway.
pub fn establish(
    asking: &Asking<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Probed, EngineError> {
    let Asking {
        workspace,
        discovery,
        sources,
        options,
    } = *asking;
    let sites = sites_of(discovery);
    if sites.is_empty() {
        return Ok(Probed::default());
    }
    let phase = trace.phase("probe");
    let root = workspace.snapshot_root().to_path_buf();
    let count = count_of(discovery);
    let catalog = discovery.catalog.digest().to_owned();
    let running = Running {
        catalog: &catalog,
        count,
        options,
    };
    let settled = accept(
        &Tree {
            workspace,
            root: &root,
            sources,
        },
        sites,
        &running,
        cancel,
    );
    let mut probed = match settled {
        Ok((accepted, files)) => {
            let outcome = build_and_run(workspace, &running, cancel, trace);
            restore(&root, sources)?;
            match outcome {
                Ok(mut probed) => {
                    probed.asked = accepted;
                    drop(files);
                    probed
                }
                Err(limitation) => Probed {
                    limitations: vec![limitation],
                    ..Probed::default()
                },
            }
        }
        Err(limitation) => {
            restore(&root, sources)?;
            Probed {
                limitations: vec![limitation],
                ..Probed::default()
            }
        }
    };
    probed.limitations.sort();
    probed.limitations.dedup();
    trace.note(
        "probe",
        &format!(
            "{} probed, {} targets",
            probed.asked.len(),
            probed.infected.len()
        ),
    );
    phase.end();
    Ok(probed)
}

/// The snapshot the probe tree is written into, and what it is written from.
#[derive(Debug, Clone, Copy)]
struct Tree<'a> {
    workspace: &'a Workspace,
    root: &'a Path,
    sources: &'a BTreeMap<String, Vec<u8>>,
}

/// Writes the probe tree, and drops every site the compiler refuses until what is left compiles.
///
/// A refused site is one this release cannot ask about — a float, a type with
/// no `Default`, a borrow the probe would outlive — and dropping it is the
/// whole point of asking. A round that fails with no site to blame is one this
/// release cannot narrow, and the pass states a limitation rather than
/// guessing.
fn accept(
    tree: &Tree<'_>,
    mut sites: BTreeMap<String, Vec<Site>>,
    running: &Running<'_>,
    cancel: &Cancel,
) -> Result<(BTreeSet<u32>, Vec<ProbedFile>), String> {
    let workspace = tree.workspace;
    for _round in 0..running.options.max_rounds.max(1) {
        let files = write(tree, &sites, running.catalog, running.count)
            .map_err(|error| error.to_string())?;
        let checked = compile(
            &workspace.driver(cancel),
            &CompileOptions {
                kind: CompileKind::Check,
                packages: Vec::new(),
                target_dir: Some(workspace.target_dir().join("probe")),
                locked: workspace.locked,
                offline: workspace.offline,
                timeout: Workspace::timeout(running.options.build_timeout),
                env: Vec::new(),
                build: running.options.build.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
        if checked.success {
            let accepted = files
                .iter()
                .flat_map(|file| file.placed.iter().map(|(_, index)| *index))
                .collect();
            return Ok((accepted, files));
        }
        let refused = refused_by(&files, &checked.messages);
        if refused.is_empty() {
            return Err(UNBUILDABLE.to_owned());
        }
        sites = without(sites, &refused);
        if sites.is_empty() {
            return Ok((BTreeSet::new(), Vec::new()));
        }
    }
    Err(UNBUILDABLE.to_owned())
}

/// The sites left once every refused one is gone, with the slots renumbered: the runtime remembers one flag per site of its own file.
fn without(
    sites: BTreeMap<String, Vec<Site>>,
    refused: &BTreeSet<u32>,
) -> BTreeMap<String, Vec<Site>> {
    let mut left: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for (path, file) in sites {
        let kept: Vec<Site> = file
            .into_iter()
            .filter(|site| !refused.contains(&site.index))
            .enumerate()
            .map(|(slot, site)| Site { slot, ..site })
            .collect();
        if !kept.is_empty() {
            left.insert(path, kept);
        }
    }
    left
}

/// The sites a diagnostic landed in. A probe the compiler refused is a probe this release does not state.
fn refused_by(files: &[ProbedFile], messages: &[crate::cargo::Message]) -> BTreeSet<u32> {
    let mut refused = BTreeSet::new();
    for message in messages {
        let crate::cargo::Message::CompilerMessage(compiler) = message else {
            continue;
        };
        if !compiler.message.is_error() {
            continue;
        }
        let Some(span) = compiler.message.primary_span() else {
            continue;
        };
        let Some(file) = files
            .iter()
            .find(|file| ends_with(&span.file_name, &file.path))
        else {
            continue;
        };
        for (placed, index) in &file.placed {
            if placed.start <= span.byte_start && span.byte_start < placed.end {
                refused.insert(*index);
            }
        }
    }
    refused
}

/// Whether the path a diagnostic names is the file that was written.
fn ends_with(reported: &str, path: &str) -> bool {
    let reported = reported.replace('\\', "/");
    reported == path || reported.ends_with(&format!("/{path}"))
}

/// How many mutants the catalog holds, which is what a log's header is checked against.
fn count_of(discovery: &Discovery) -> u32 {
    u32::try_from(discovery.catalog.mutants().len()).unwrap_or(u32::MAX)
}

/// Every site a probe can be stated for, by the file it is in.
fn sites_of(discovery: &Discovery) -> BTreeMap<String, Vec<Site>> {
    let mut sites: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for located in &discovery.candidates {
        let Some(question) = located.found.probe else {
            continue;
        };
        let Ok(id) = located.found.candidate.id() else {
            continue;
        };
        let Some(mutant) = discovery.catalog.by_id(&id) else {
            continue;
        };
        let file = sites
            .entry(located.found.candidate.path.clone())
            .or_default();
        if file
            .iter()
            .any(|site| site.span == located.found.candidate.span)
        {
            continue;
        }
        let slot = file.len();
        file.push(Site {
            index: mutant.index,
            slot,
            span: located.found.candidate.span,
            question,
            super_depth: located.found.hint.super_depth,
        });
    }
    sites.retain(|_path, file| !file.is_empty());
    sites
}

/// Writes the probe tree over the pristine sources.
fn write(
    tree: &Tree<'_>,
    sites: &BTreeMap<String, Vec<Site>>,
    catalog: &str,
    count: u32,
) -> Result<Vec<ProbedFile>, EngineError> {
    let Tree { root, sources, .. } = *tree;
    let mut written = Vec::new();
    for (path, file) in sites {
        let Some(source) = sources.get(path) else {
            continue;
        };
        let Some(probed) = probe_file(&Reading { path, source }, file, catalog, count) else {
            continue;
        };
        std::fs::write(root.join(path), &probed.text).map_err(|source| {
            SessionError::WriteFailed {
                path: path.clone(),
                source,
            }
        })?;
        written.push(probed);
    }
    Ok(written)
}

/// One file as it is.
#[derive(Debug, Clone, Copy)]
struct Reading<'a> {
    path: &'a str,
    source: &'a [u8],
}

/// One file with every probeable site rewritten, or nothing when the rewrite would move a line.
fn probe_file(file: &Reading<'_>, sites: &[Site], catalog: &str, count: u32) -> Option<ProbedFile> {
    let Reading { path, source } = *file;
    let text = String::from_utf8_lossy(source).into_owned();
    let module = crate::instrument::module_named_for(&text, MODULE_STEM);
    let mut splices = Vec::new();
    for site in sites {
        let original = source.get(at(site.span.start)..at(site.span.end))?;
        let expression = std::str::from_utf8(original).ok()?;
        splices.push(Splice {
            span: site.span,
            original: original.to_vec(),
            replacement: rewrite(site, expression, &module).into_bytes(),
        });
    }
    let (bytes, map) = apply(source, &splices).ok()?;
    if count_lines(&bytes) != count_lines(source) {
        return None;
    }
    let mut rewritten = String::from_utf8_lossy(&bytes).into_owned();
    if !rewritten.ends_with('\n') {
        rewritten.push('\n');
    }
    let indices: Vec<u32> = sites.iter().map(|site| site.index).collect();
    rewritten.push_str(&runtime::render(&module, catalog, count, &indices));
    let placed = sites
        .iter()
        .zip(&splices)
        .map(|(site, splice)| {
            let start = map.to_output(splice.span.start).0;
            (
                Span {
                    start,
                    end: start.saturating_add(
                        u32::try_from(splice.replacement.len()).unwrap_or(u32::MAX),
                    ),
                },
                site.index,
            )
        })
        .collect();
    Some(ProbedFile {
        path: path.to_owned(),
        text: rewritten,
        placed,
    })
}

/// What the probe runs are about.
#[derive(Debug, Clone, Copy)]
struct Running<'a> {
    catalog: &'a str,
    count: u32,
    options: &'a PrepareOptions,
}

/// Builds the probe tree and runs every test in it, reading what each infected.
fn build_and_run(
    workspace: &Workspace,
    running: &Running<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Probed, String> {
    let Running {
        catalog,
        count,
        options,
    } = *running;
    let target_dir = workspace.target_dir().join("probe");
    let built = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: options.packages.clone(),
            target_dir: Some(target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: Vec::new(),
            build: options.build.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    if !built.success {
        return Err(UNBUILDABLE.to_owned());
    }
    let logs = target_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let targets = execute::targets_of(
        &built.messages,
        &workspace.metadata().packages,
        Some(&target_dir),
    );
    let mut probed = Probed::default();
    for target in &targets {
        let log_path = logs.join(format!("{}.log", target.id.replace('/', "-")));
        let context = Context {
            base_env: &workspace.base_env,
            cargo: Some(workspace.toolchain().cargo()),
            sysroot: workspace.toolchain().sysroot(),
            active: None,
            probe: Some(&log_path),
            touch: None,
            profile: None,
        };
        let (budget, _source) = options.mutant_timeout.of(None);
        let request = ExecRequest::new(target).with_timeout(Some(budget));
        let result = execute::exec(&request, &context, cancel, trace);
        let duration_ms = u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX);
        let record = |outcome: &str, infected: Option<u32>| {
            trace.probe_exec(crate::trace::ProbeExecRecord {
                target: target.id.clone(),
                outcome: outcome.to_owned(),
                infected,
                duration_ms,
            });
        };
        if result.exit_code == runtime::UNAVAILABLE_EXIT {
            record("unreadable-log", None);
            probed.limitations.push(UNREADABLE_LOG.to_owned());
            continue;
        }
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        match log::read(&text, catalog, count) {
            Ok(infected) => {
                record(
                    "measured",
                    Some(u32::try_from(infected.len()).unwrap_or(u32::MAX)),
                );
                probed.infected.insert(target.id.clone(), infected);
            }
            Err(_unreadable) => {
                record("unreadable-log", None);
                probed.limitations.push(UNREADABLE_LOG.to_owned());
            }
        }
    }
    Ok(probed)
}

/// Puts the pristine sources back.
fn restore(root: &Path, sources: &BTreeMap<String, Vec<u8>>) -> Result<(), EngineError> {
    for (path, source) in sources {
        std::fs::write(root.join(path), source).map_err(|error| SessionError::WriteFailed {
            path: path.clone(),
            source: error,
        })?;
    }
    Ok(())
}

fn at(offset: u32) -> usize {
    usize::try_from(offset).unwrap_or(usize::MAX)
}

/// Where the probe pass keeps the logs it reads.
#[must_use]
pub fn logs_dir(target_dir: &Path) -> PathBuf {
    target_dir.join("probe").join("logs")
}
