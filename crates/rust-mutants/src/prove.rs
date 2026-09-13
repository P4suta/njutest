// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting a branch proof's claims to the compiler, and keeping only what it accepts.
//!
//! The claims discovery makes rest on the whole condition being inert, and the
//! one thing syntax cannot decide about that is whether the operands are
//! primitives. The witness tree asks: it writes the pristine sources with a
//! witness statement in front of each condition, runs `cargo check`, refuses
//! every claim a diagnostic lands in, and puts the pristine sources back.
//!
//! Nothing here can make a run fail. A witness tree that will not build for a
//! reason no claim accounts for means this release proves nothing about that
//! workspace, which costs time and never correctness.

use std::collections::{BTreeMap, BTreeSet};
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
///
/// The proof is the compiler's: this mutation changes nothing outside the
/// body the condition gates. The premise is the measurement's: during this
/// target's run, no block of that body was executed. Together they say the
/// target cannot have observed the mutation, so running it proves nothing and
/// costs a process.
///
/// It is a pure function of the two, so a caller with its own coverage can
/// discharge with its own evidence and an audit can re-derive the decision
/// without the engine.
#[must_use]
pub fn discharges(proof: &Proof, path: &Path, covered: &[crate::coverage::Block]) -> bool {
    !covered.iter().any(|block| in_body(proof, path, block))
}

/// Whether one covered block begins inside the body a proof names.
///
/// Coverage regions nest, and what says the body ran is a region that
/// *begins* inside it. The region of the function that holds the branch
/// contains the body and says the function ran; the region that begins at the
/// body's closing brace is the one the compiler emits for what follows the
/// branch, and a run that went past the branch without taking it has it. So
/// the body is `[opening brace, closing brace)`: a region at the closing
/// brace is not the body's.
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
/// Only a failure to write the tree or to put it back, which leaves the
/// snapshot in a state no later phase could trust.
pub fn establish(
    asking: &Asking<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Established, EngineError> {
    establish_for(asking, None, cancel, trace)
}

/// Establishes proofs only for the catalog indices a prepared run can use.
///
/// Discovery and identity remain global. Narrowing only the witness questions
/// prevents a scoped run from paying a whole-catalog compiler pass for proofs
/// it cannot consume.
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
        workspace,
        discovery,
        sources,
        options,
    } = *asking;
    let questions = questions_of(discovery, selected);
    if questions.is_empty() {
        return Ok(Established::default());
    }
    let claims = &questions.conditions;
    let _phase = trace.phase("witness");
    let root = workspace.snapshot_root().to_path_buf();
    let wrote = write(&root, sources, &questions);
    let checked = wrote
        .is_ok()
        .then(|| compile(&workspace.driver(cancel), &checking(workspace, options)));
    restore(&root, sources)?;
    let written = wrote?;

    let Some(Ok(checked)) = checked else {
        return Ok(Established::default());
    };
    let mut refused = if checked.success {
        Refusal::default()
    } else {
        refusal(&written.files, &checked.messages)
    };
    if !checked.success && !refused.accounts_for_a_failure() {
        trace.note(
            "witness",
            &format!(
                "the witness tree did not compile and no rewrite of it accounts for that, so \
                 nothing is vouched for: {}",
                refused
                    .unaccounted
                    .first()
                    .map_or("the compiler named no place at all", String::as_str)
            ),
        );
        return Ok(Established::default());
    }
    unasked(&questions, &written.unasked, &mut refused);
    let markers = markers_of(claims);
    let established = vouched(&questions, sources, &Checked { refused, markers }, trace);
    trace.note(
        "witness",
        &format!(
            "{} claimed of which {} name a body, {} vouched for of which {} carry a marker; \
             {} values probed of which {} vouched for",
            count(claims),
            claims
                .values()
                .flatten()
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
    Ok(established)
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
) -> Established {
    let Checked { refused, markers } = checked;
    let mut established = Established::default();
    for file in questions.probes.values() {
        for probe in file {
            if !refused.probes.contains(&probe.index) {
                let _vouched = established.probed.insert(probe.index, probe.question);
            }
        }
    }
    for (path, file) in &questions.conditions {
        let Some(source) = sources.get(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(source);
        let index = LineIndex::new(&text);
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
            let _vouched = established.comparable.insert(claimed.index);
            let Some(body) = claimed.body else {
                continue;
            };
            let _kept = established.proofs.insert(
                claimed.index,
                Proof {
                    body_start: index.position(&text, body.start),
                    body_end: end_of(&index, &text, body.end),
                    marker: markers
                        .get(&(path.clone(), body))
                        .copied()
                        .filter(|_| !refused.markers.contains(&claimed.index)),
                },
            );
        }
    }
    established
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

/// The position one past the body's last byte. A body's end is exclusive, and a reader looking at the closing brace wants where it is rather than where the next thing starts.
fn end_of(index: &LineIndex, text: &str, offset: u32) -> Position {
    index.position(text, offset.saturating_sub(1))
}

/// The marker each body would carry, by the file and the body it is in.
///
/// One body carries one marker however many claims rest on it, and it names
/// the lowest of them: an index out of the catalog's own numbering, so the log
/// that records it needs no second numbering to bound.
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
        let Some(mutant) = discovery.catalog.by_id(&id) else {
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
///
/// A branch proof and a comparison rest on the same thing — the condition
/// being inert — and are written in front of the same bytes. The proof asks
/// for more: it names the body the condition gates, which a comparison has no
/// use for. So a candidate with a proof carries the body and one with only a
/// comparison does not, and both are one rewrite of one condition.
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
///
/// The check builds into a directory of its own so that the witness tree's
/// artifacts do not displace the run's. Both trees name the same crates, so
/// one target directory would hold whichever was compiled last, and every
/// build after a witness pass would be a build of everything again.
fn checking(workspace: &Workspace, options: &PrepareOptions) -> CompileOptions {
    CompileOptions {
        kind: CompileKind::Check,
        packages: Vec::new(),
        target_dir: Some(workspace.target_dir.join("witness")),
        locked: workspace.locked,
        offline: workspace.offline,
        timeout: Workspace::timeout(options.build_timeout),
        env: Vec::new(),
        build: options.build.clone(),
    }
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
            let _unasked = written.unasked.insert(path.clone());
            continue;
        };
        let Ok(one) = witness::witness_file(path, source, &questions.of(path, &empty)) else {
            let _unasked = written.unasked.insert(path.clone());
            continue;
        };
        if !one.witnessed {
            let _unasked = written.unasked.insert(path.clone());
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
///
/// A file the tree could not be given is a file the compiler was never asked
/// about, and a check that passes over it says nothing about anything in it.
/// Reading that silence as acceptance would grant every claim and every probe
/// of the file on a question nobody put — which is the one direction a proof
/// layer may never fail in.
#[derive(Debug, Default)]
struct Written {
    /// The files the tree holds, which the diagnostics are read against.
    files: Vec<witness::WitnessFile>,
    /// The files it does not, whose claims and probes are refused for that reason alone.
    unasked: BTreeSet<String>,
}

/// Puts the pristine sources back. A tree left witnessed is one every later phase would be about the wrong program.
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
///
/// An error inside a condition's witnesses refuses that claim; one inside a
/// body's marker refuses only the marker. An error that lands in neither is
/// one this rule cannot localise, and it is reported rather than passed over:
/// a check that failed for a reason nothing accounts for is a check that says
/// nothing about any claim, and reading it as "nothing was refused" would
/// grant every one of them on a question the compiler never answered.
///
/// A diagnostic with no span at all is cargo's own summary of the ones that
/// have them, and says nothing this rule does not already have.
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
///
/// A diagnostic in a condition's witnesses refuses the claim, because the
/// whole of it rests on the compiler accepting them. One in a body's marker
/// refuses only the marker: the claim stands, with a coverage region as the
/// one premise left to establish it. An error in neither costs every claim,
/// because it is the compiler refusing the tree for a reason this pass cannot
/// name.
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

impl Refusal {
    /// Whether a check that failed is one this rule accounted for, which is what makes what it did not refuse a thing the compiler took.
    ///
    /// Something has to have been refused. A check the compiler failed and
    /// this rule found nothing wrong with is one whose reason lies outside
    /// every rewrite — in the manifest, the linker, a lint the tree denies —
    /// and the claims it says nothing about are claims, not proofs.
    #[must_use]
    pub fn accounts_for_a_failure(&self) -> bool {
        self.unaccounted.is_empty()
            && !(self.claims.is_empty() && self.markers.is_empty() && self.probes.is_empty())
    }
}

/// Refuses every claim and every probe of a file the witness tree does not hold.
///
/// The compiler was asked nothing about them, and what nobody asked about is
/// not something the compiler took.
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
