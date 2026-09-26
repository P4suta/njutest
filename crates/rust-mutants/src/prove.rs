// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting a branch proof's claims to the compiler, and keeping only what it accepts.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::discover::Discovery;
use crate::instrument::witness::{self, Claimed};
use crate::runner::Cancel;
use crate::session::PrepareOptions;
use crate::syntax::branch::{Marker, Proof};
use crate::syntax::{LineIndex, Position};
use crate::trace::Recorder;
use crate::workspace::{SessionError, Workspace};

/// What one file's claims are, in catalog order.
type ByFile = BTreeMap<String, Vec<Claimed>>;

/// What one file's probes are, in catalog order.
type ProbesByFile = BTreeMap<String, Vec<witness::Probing>>;

/// Everything one `cargo check` of the witness tree is asked, by file.
#[derive(Debug, Clone, Default)]
struct Questions {
    /// The conditions a branch proof or a comparison rests on.
    conditions: ByFile,
    /// The returned values a probe rests on.
    probes: ProbesByFile,
}

impl Questions {
    /// Whether the tree is asked anything at all.
    fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.probes.is_empty()
    }

    /// Every file either question is asked about.
    fn paths(&self) -> BTreeSet<&String> {
        self.conditions.keys().chain(self.probes.keys()).collect()
    }

    /// The questions left once `refused` is taken out: no refused claim or probe, and no marker in a body that would not take one.
    fn without(&self, refused: &Refusal) -> Self {
        let conditions = self
            .conditions
            .iter()
            .map(|(path, file)| {
                let kept: Vec<Claimed> = file
                    .iter()
                    .filter(|claimed| !refused.claims.contains(&claimed.index))
                    .map(|claimed| {
                        let mut kept = claimed.clone();
                        if refused.markers.contains(&claimed.index) {
                            kept.body = None;
                        }
                        kept
                    })
                    .collect();
                (path.clone(), kept)
            })
            .filter(|(_, kept)| !kept.is_empty())
            .collect();
        let probes = self
            .probes
            .iter()
            .map(|(path, file)| {
                let kept: Vec<witness::Probing> = file
                    .iter()
                    .copied()
                    .filter(|probe| !refused.probes.contains(&probe.index))
                    .collect();
                (path.clone(), kept)
            })
            .filter(|(_, kept)| !kept.is_empty())
            .collect();
        Self { conditions, probes }
    }

    /// What one file is asked.
    fn of<'a>(&'a self, path: &str, empty: &'a Empty) -> witness::Asking<'a> {
        witness::Asking {
            conditions: self
                .conditions
                .get(path)
                .map_or(&empty.conditions, Vec::as_slice),
            probes: self.probes.get(path).map_or(&empty.probes, Vec::as_slice),
        }
    }
}

/// What a file that is asked only one of the two questions is handed for the other.
#[derive(Debug, Default)]
struct Empty {
    conditions: Vec<Claimed>,
    probes: Vec<witness::Probing>,
}

/// What the question is put about: the tree, what discovery claimed, and how the check is bounded.
#[derive(Debug, Clone, Copy)]
pub struct Asking<'a> {
    /// The snapshot the witness tree is written into and taken out of.
    pub workspace: &'a Workspace,
    /// What discovery claimed.
    pub discovery: &'a Discovery,
    /// The pristine bytes of every file that yielded a candidate.
    pub sources: &'a BTreeMap<String, Vec<u8>>,
    /// How the check is bounded.
    pub options: &'a PrepareOptions,
}

/// Whether a target that covered `covered` could not have noticed a mutation whose branch proof is `proof`.
#[must_use]
pub fn discharges(proof: &Proof, path: &Path, covered: &[crate::coverage::Block]) -> bool {
    !covered.iter().any(|block| in_body(proof, path, block))
}

/// Whether one covered block begins inside the body a proof names.
fn in_body(proof: &Proof, path: &Path, block: &crate::coverage::Block) -> bool {
    if Path::new(&block.file) != path {
        return false;
    }
    let start = (block.start.line, block.start.column);
    let body_start = (proof.body_start.line, proof.body_start.byte_column);
    let body_end = (proof.body_end.line, proof.body_end.byte_column);
    start >= body_start && start < body_end
}

/// Every mutant whose branch proof the compiler accepted, with the proof.
///
/// # Errors
/// Only a failure to write the tree or to put it back, which leaves the snapshot in a state no later phase could trust.
pub fn establish(
    asking: &Asking<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Established, EngineError> {
    establish_for(asking, None, cancel, trace)
}

/// Establishes proofs only for the catalog indices a prepared run can use.
///
/// # Errors
/// The same failures as [`establish`].
pub fn establish_selected(
    asking: &Asking<'_>,
    selected: &BTreeSet<u32>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Established, EngineError> {
    establish_for(asking, Some(selected), cancel, trace)
}

fn establish_for(
    asking: &Asking<'_>,
    selected: Option<&BTreeSet<u32>>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Established, EngineError> {
    let Asking {
        discovery, sources, ..
    } = *asking;
    let questions = questions_of(discovery, selected);
    if questions.is_empty() {
        return Ok(Established::default());
    }
    let claims = &questions.conditions;
    let witness_phase = trace.phase("witness");
    let Some(Vouching {
        written,
        mut refused,
        checks,
    }) = checked_until_compiled(asking, &questions, cancel, trace)?
    else {
        drop(witness_phase);
        return Ok(Established::default());
    };
    unasked(&questions, &written.unasked, &mut refused);
    let markers = markers_of(claims);
    let established = vouched(&questions, sources, &Checked { refused, markers }, trace)?;
    trace.note(
        "witness",
        &format!(
            "{} claimed of which {} name a body, {} vouched for of which {} carry a marker; \
             {} values probed of which {} vouched for; check {checks} was the first to compile \
             every target",
            count(claims),
            claims
                .values()
                .flat_map(|claims| claims.iter())
                .filter(|claimed| claimed.body.is_some())
                .count(),
            established.comparable.len(),
            established
                .proofs
                .values()
                .filter(|proof| proof.marker.is_some())
                .count(),
            questions.probes.values().map(Vec::len).sum::<usize>(),
            established.probed.len(),
        ),
    );
    drop(witness_phase);
    Ok(established)
}

/// What the checks of the witness tree came to: the tree the one that compiled held, everything refused on the way, and how many checks it took.
struct Vouching {
    written: Written,
    refused: Refusal,
    checks: u32,
}

/// Checks the witness tree, each time without what the check before refused, until one compiles every target, which is the only check that vouches for what it held.
/// A crate that fails stops cargo before the crates that depend on it, so a failed check has said nothing about their witnesses; nothing is vouched for when a failure is not one this rule accounts for, or when the checks run out.
fn checked_until_compiled(
    asking: &Asking<'_>,
    questions: &Questions,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Option<Vouching>, EngineError> {
    let Asking {
        workspace,
        sources,
        options,
        ..
    } = *asking;
    let root = workspace.snapshot_root().to_path_buf();
    let mut refused = Refusal::default();
    let mut asked = questions.clone();
    for checks in 1..=WITNESS_CHECKS {
        let wrote = write(&root, sources, &asked);
        let checked = if wrote.is_ok() {
            Some(compile(
                &workspace.driver(cancel),
                &checking(workspace, options)?,
            ))
        } else {
            None
        };
        restore(&root, sources)?;
        let written = wrote?;
        let Some(Ok(checked)) = checked else {
            return Ok(None);
        };
        if checked.success {
            return Ok(Some(Vouching {
                written,
                refused,
                checks,
            }));
        }
        let round = refusal(&written.files, &checked.messages);
        if !round.accounts_for_a_failure() {
            trace.note(
                "witness",
                &format!(
                    "the witness tree did not compile and no rewrite of it accounts for that, so \
                     nothing is vouched for: {}",
                    round
                        .unaccounted
                        .first()
                        .map_or("the compiler named no place at all", String::as_str)
                ),
            );
            return Ok(None);
        }
        refused.absorb(round);
        asked = questions.without(&refused);
    }
    trace.note(
        "witness",
        &format!(
            "the witness tree still did not compile after {WITNESS_CHECKS} checks, each without \
             what the one before refused, so nothing is vouched for"
        ),
    );
    Ok(None)
}

/// What the one `cargo check` established: what it would not take, and the marker each body carries.
struct Checked {
    /// What the compiler would not take, by what it costs.
    refused: Refusal,
    /// The marker each body would carry, by the file and the body it is in.
    markers: BTreeMap<(String, crate::span::Span), Marker>,
}

/// Every claim the compiler took, with the body it names and the marker that body carries.
fn vouched(
    questions: &Questions,
    sources: &BTreeMap<String, Vec<u8>>,
    checked: &Checked,
    trace: &Recorder,
) -> Result<Established, SessionError> {
    let Checked { refused, markers } = checked;
    let mut established = Established::default();
    for file in questions.probes.values() {
        for probe in file {
            if !refused.probes.contains(&probe.index) {
                established
                    .probed
                    .entry(probe.index)
                    .or_insert(probe.question);
            }
        }
    }
    for (path, file) in &questions.conditions {
        let source = sources
            .get(path)
            .ok_or_else(|| SessionError::SelectionSourceMissing { path: path.clone() })?;
        let text =
            std::str::from_utf8(source).map_err(|source| SessionError::SelectionSourceNotUtf8 {
                path: path.clone(),
                source,
            })?;
        let index =
            LineIndex::new(text).map_err(|source| SessionError::SelectionPositionInvalid {
                path: path.clone(),
                source,
            })?;
        for claimed in file {
            trace.witness(crate::trace::WitnessRecord {
                index: claimed.index,
                witnesses: claimed
                    .witnesses
                    .iter()
                    .map(|witness| witness.kind.function().to_owned())
                    .collect(),
                checked: !refused.claims.contains(&claimed.index),
                diagnostic: None,
            });
            if refused.claims.contains(&claimed.index) {
                continue;
            }
            established
                .comparable
                .extend(std::iter::once(claimed.index));
            let Some(body) = claimed.body else {
                continue;
            };
            let proof = Proof {
                body_start: index.position(body.start).map_err(|source| {
                    SessionError::SelectionPositionInvalid {
                        path: path.clone(),
                        source,
                    }
                })?,
                body_end: end_of(&index, body.end).map_err(|source| {
                    SessionError::SelectionPositionInvalid {
                        path: path.clone(),
                        source,
                    }
                })?,
                marker: markers
                    .get(&(path.clone(), body))
                    .copied()
                    .filter(|_| !refused.markers.contains(&claimed.index)),
            };
            established.proofs.entry(claimed.index).or_insert(proof);
        }
    }
    Ok(established)
}

/// What the one `cargo check` established about the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Established {
    /// Every mutant whose branch proof the compiler accepted, with the proof.
    pub proofs: BTreeMap<u32, Proof>,
    /// Every mutant whose guard may compare its two branches, because the compiler vouched for the condition being inert.
    pub comparable: BTreeSet<u32>,
    /// Every return replacement whose guard may ask whether the value it replaces already holds what it would write, with the question to ask.
    pub probed: BTreeMap<u32, crate::probe::Question>,
}

/// The position one past the body's last byte.
/// A body's end is exclusive, and a reader looking at the closing brace wants where it is rather than where the next thing starts.
fn end_of(index: &LineIndex<'_>, offset: u32) -> Result<Position, crate::syntax::PositionError> {
    let last = match offset.checked_sub(1) {
        Some(last) => last,
        None => 0,
    };
    index.position(last)
}

/// The marker each body would carry, by the file and the body it is in.
fn markers_of(claims: &ByFile) -> BTreeMap<(String, crate::span::Span), Marker> {
    let mut bodies: BTreeMap<(String, crate::span::Span), Marker> = BTreeMap::new();
    for (path, file) in claims {
        for claimed in file {
            let Some(body) = claimed.body else {
                continue;
            };
            let marker = Marker {
                at: body.start.saturating_add(1),
                index: claimed.index,
                super_depth: claimed.super_depth,
            };
            bodies
                .entry((path.clone(), body))
                .and_modify(|held| held.index = held.index.min(claimed.index))
                .or_insert(marker);
        }
    }
    bodies
}

fn count(claims: &ByFile) -> usize {
    claims.values().map(Vec::len).sum()
}

/// Every claim discovery made, by the file it is in.
fn questions_of(discovery: &Discovery, selected: Option<&BTreeSet<u32>>) -> Questions {
    let mut questions = Questions::default();
    for located in &discovery.candidates {
        let Ok(id) = located.found.candidate.id() else {
            continue;
        };
        let Some(mutant) = discovery.catalog.by_id(id.as_str()) else {
            continue;
        };
        if selected.is_some_and(|selected| !selected.contains(&mutant.index)) {
            continue;
        }
        if let Some(question) = located.found.probe {
            questions
                .probes
                .entry(located.found.candidate.path.clone())
                .or_default()
                .push(witness::Probing {
                    index: mutant.index,
                    value: located.found.hint.site,
                    question,
                    super_depth: located.found.hint.super_depth,
                });
        }
        let Some(rests) = rests_on(&located.found) else {
            continue;
        };
        questions
            .conditions
            .entry(located.found.candidate.path.clone())
            .or_default()
            .push(Claimed {
                index: mutant.index,
                condition: rests.condition,
                body: rests.body,
                witnesses: rests.witnesses,
                super_depth: located.found.hint.super_depth,
            });
    }
    questions
}

/// What one candidate has to put to the compiler, or nothing when it asks nothing of it.
fn rests_on(found: &crate::syntax::Found) -> Option<Rests> {
    if let Some(claim) = &found.branch {
        return Some(Rests {
            condition: claim.condition,
            body: Some(claim.body),
            witnesses: claim.witnesses.clone(),
        });
    }
    let comparable = found.comparable.as_ref()?;
    Some(Rests {
        condition: comparable.condition,
        body: None,
        witnesses: comparable.witnesses.clone(),
    })
}

/// What one candidate rests on: the condition to witness, and the body a branch proof about it names.
struct Rests {
    /// The whole condition, which the witnesses are written in front of.
    condition: crate::span::Span,
    /// The body a branch proof names, or nothing where only the comparison rests on the condition.
    body: Option<crate::span::Span>,
    /// What the compiler must vouch for.
    witnesses: Vec<crate::syntax::branch::Witness>,
}

/// How the witness tree is checked.
fn checking(
    workspace: &Workspace,
    options: &PrepareOptions,
) -> Result<CompileOptions, crate::cargo::config::ConfigError> {
    Ok(CompileOptions {
        kind: CompileKind::Check,
        packages: Vec::new(),
        target_dir: Some(workspace.target_dir.join("witness")),
        locked: workspace.locked,
        offline: workspace.offline,
        timeout: Workspace::timeout(options.build_timeout),
        env: capping(workspace)?,
        build: options.build.clone(),
    })
}

/// The flag that holds every lint to a warning, spelled without a space so it survives every form of the variable.
const CAP_LINTS: &str = "--cap-lints=warn";

/// The compiler flags the witness check adds to what the workspace is otherwise compiled with.
fn capping(
    workspace: &Workspace,
) -> Result<Vec<(OsString, OsString)>, crate::cargo::config::ConfigError> {
    let flags = crate::cargo::config::configured(
        workspace.snapshot_root(),
        crate::cargo::config::home(&workspace.base_env).as_deref(),
    );
    let encoded = match crate::cargo::config::encoded(&workspace.base_env, &flags, &[CAP_LINTS])? {
        Some(encoded) => encoded,
        None => OsString::new(),
    };
    Ok(vec![
        (
            OsString::from(crate::cargo::config::ENCODED_RUSTFLAGS),
            encoded,
        ),
        (
            OsString::from(crate::cargo::config::RUSTFLAGS),
            OsString::new(),
        ),
    ])
}

/// Writes the witness tree over the pristine sources.
fn write(
    root: &Path,
    sources: &BTreeMap<String, Vec<u8>>,
    questions: &Questions,
) -> Result<Written, EngineError> {
    let empty = Empty::default();
    let mut written = Written::default();
    for path in questions.paths() {
        let Some(source) = sources.get(path) else {
            written.unasked.extend(std::iter::once(path.clone()));
            continue;
        };
        let Ok(one) = witness::witness_file(path, source, &questions.of(path, &empty)) else {
            written.unasked.extend(std::iter::once(path.clone()));
            continue;
        };
        if !one.witnessed {
            written.unasked.extend(std::iter::once(path.clone()));
            continue;
        }
        std::fs::write(root.join(path), &one.text).map_err(|source| SessionError::WriteFailed {
            path: path.clone(),
            source,
        })?;
        written.files.push(one);
    }
    Ok(written)
}

/// What the witness tree came to: the files it holds, and the ones it does not.
#[derive(Debug, Default)]
struct Written {
    /// The files the tree holds, which the diagnostics are read against.
    files: Vec<witness::WitnessFile>,
    /// The files it does not, whose claims and probes are refused for that reason alone.
    unasked: BTreeSet<String>,
}

/// Puts the pristine sources back.
/// A tree left witnessed is one every later phase would be about the wrong program.
fn restore(root: &Path, sources: &BTreeMap<String, Vec<u8>>) -> Result<(), EngineError> {
    for (path, source) in sources {
        std::fs::write(root.join(path), source).map_err(|error| SessionError::WriteFailed {
            path: path.clone(),
            source: error,
        })?;
    }
    Ok(())
}

/// What a failed check of the witness tree costs, as the pure rule of the diagnostics and the rewrites they landed in.
#[must_use]
pub fn refusal(written: &[witness::WitnessFile], messages: &[crate::cargo::Message]) -> Refusal {
    let mut refused = Refusal::default();
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
        let landed = written
            .iter()
            .find(|file| crate::cargo::names_file(&span.file_name, &file.path))
            .into_iter()
            .flat_map(|file| &file.sites)
            .filter(|site| site.span.start <= span.byte_start && span.byte_start < site.span.end)
            .fold(false, |_, site| {
                match site.placed {
                    witness::Placed::Witnesses => {
                        refused.claims.extend(site.claims.iter().copied());
                    }
                    witness::Placed::Marker => {
                        refused.markers.extend(site.claims.iter().copied());
                    }
                    witness::Placed::Probe => {
                        refused.probes.extend(site.claims.iter().copied());
                    }
                }
                true
            });
        if !landed {
            refused
                .unaccounted
                .push(format!("{}: {}", span.file_name, compiler.message.message));
        }
    }
    refused
}

/// What the compiler would not take, by what it costs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    /// The mutants whose claim the compiler would not take.
    pub claims: BTreeSet<u32>,
    /// The mutants whose body the compiler would not take a marker in.
    pub markers: BTreeSet<u32>,
    /// The mutants whose returned value is of a type no guard may compare against what a replacement writes.
    pub probes: BTreeSet<u32>,
    /// Every error no rewrite of the witness tree accounts for, as the file and what the compiler said.
    pub unaccounted: Vec<String>,
}

/// How many checks of the witness tree may run, each without what the one before refused, before nothing is vouched for.
/// A crate that fails stops cargo before the crates that depend on it, so a workspace needs one more check for every layer of its dependency graph whose witnesses the compiler refuses.
const WITNESS_CHECKS: u32 = 8;

impl Refusal {
    /// Takes in what one more check refused.
    fn absorb(&mut self, round: Self) {
        self.claims.extend(round.claims);
        self.markers.extend(round.markers);
        self.probes.extend(round.probes);
        self.unaccounted.extend(round.unaccounted);
    }

    /// Whether a check that failed is one this rule accounted for, which is what makes what it did not refuse a thing the compiler took.
    #[must_use]
    pub fn accounts_for_a_failure(&self) -> bool {
        self.unaccounted.is_empty()
            && !(self.claims.is_empty() && self.markers.is_empty() && self.probes.is_empty())
    }
}

/// Refuses every claim and every probe of a file the witness tree does not hold.
fn unasked(questions: &Questions, paths: &BTreeSet<String>, refused: &mut Refusal) {
    for path in paths {
        if let Some(file) = questions.conditions.get(path) {
            refused
                .claims
                .extend(file.iter().map(|claimed| claimed.index));
        }
        if let Some(file) = questions.probes.get(path) {
            refused.probes.extend(file.iter().map(|probe| probe.index));
        }
    }
}
