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
) -> Result<BTreeMap<u32, Proof>, EngineError> {
    let Asking {
        workspace,
        discovery,
        sources,
        options,
    } = *asking;
    let claims = claims_of(discovery);
    if claims.is_empty() {
        return Ok(BTreeMap::new());
    }
    let phase = trace.phase("witness");
    let root = workspace.snapshot_root().to_path_buf();
    let written = write(&root, sources, &claims)?;
    let checked = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(workspace.target_dir.join("witness")),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: Vec::new(),
            build: options.build.clone(),
        },
    );
    restore(&root, sources)?;

    let Ok(checked) = checked else {
        phase.end();
        return Ok(BTreeMap::new());
    };
    let refused = if checked.success {
        Refused::default()
    } else {
        refused_by(&written, &checked.messages)
    };
    let markers = markers_of(&claims);
    let said = format!(
        "{} claimed, {} refused, {} bodies the compiler would not take a marker in",
        count(&claims),
        refused.claims.len(),
        refused.markers.len()
    );
    let proofs = vouched(&claims, sources, &Checked { refused, markers }, trace);
    trace.note("witness", &said);
    phase.end();
    Ok(proofs)
}

/// What the one `cargo check` established: what it would not take, and the marker each body carries.
struct Checked {
    /// What the compiler would not take, by what it costs.
    refused: Refused,
    /// The marker each body would carry, by the file and the body it is in.
    markers: BTreeMap<(String, crate::span::Span), Marker>,
}

/// Every claim the compiler took, with the body it names and the marker that body carries.
fn vouched(
    claims: &ByFile,
    sources: &BTreeMap<String, Vec<u8>>,
    checked: &Checked,
    trace: &Recorder,
) -> BTreeMap<u32, Proof> {
    let Checked { refused, markers } = checked;
    let mut proofs = BTreeMap::new();
    for (path, file) in claims {
        let Some(source) = sources.get(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(source);
        let index = LineIndex::new(&text);
        for claimed in file {
            trace.witness(crate::trace::WitnessRecord {
                index: claimed.index,
                witnesses: claimed
                    .claim
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
            let _kept = proofs.insert(
                claimed.index,
                Proof {
                    body_start: index.position(&text, claimed.claim.body.start),
                    body_end: end_of(&index, &text, claimed.claim.body.end),
                    marker: markers
                        .get(&(path.clone(), claimed.claim.body))
                        .copied()
                        .filter(|_| !refused.markers.contains(&claimed.index)),
                },
            );
        }
    }
    proofs
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
            let marker = Marker {
                at: claimed.claim.body.start.saturating_add(1),
                index: claimed.index,
                super_depth: claimed.super_depth,
            };
            bodies
                .entry((path.clone(), claimed.claim.body))
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
fn claims_of(discovery: &Discovery) -> ByFile {
    let mut claims: ByFile = BTreeMap::new();
    for located in &discovery.candidates {
        let Some(claim) = located.found.branch.clone() else {
            continue;
        };
        let Ok(id) = located.found.candidate.id() else {
            continue;
        };
        let Some(mutant) = discovery.catalog.by_id(&id) else {
            continue;
        };
        claims
            .entry(located.found.candidate.path.clone())
            .or_default()
            .push(Claimed {
                index: mutant.index,
                claim,
                super_depth: located.found.hint.super_depth,
            });
    }
    claims
}

/// Writes the witness tree over the pristine sources.
fn write(
    root: &Path,
    sources: &BTreeMap<String, Vec<u8>>,
    claims: &ByFile,
) -> Result<Vec<witness::WitnessFile>, EngineError> {
    let mut written = Vec::new();
    for (path, file) in claims {
        let Some(source) = sources.get(path) else {
            continue;
        };
        let Ok(one) = witness::witness_file(path, source, file) else {
            continue;
        };
        if !one.witnessed {
            continue;
        }
        std::fs::write(root.join(path), &one.text).map_err(|source| SessionError::WriteFailed {
            path: path.clone(),
            source,
        })?;
        written.push(one);
    }
    Ok(written)
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

/// The claims a diagnostic landed in. A witness the compiler refused is a claim this release does not make.
fn refused_by(written: &[witness::WitnessFile], messages: &[crate::cargo::Message]) -> Refused {
    let mut refused = Refused::default();
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
        let Some(file) = written
            .iter()
            .find(|file| ends_with(&span.file_name, &file.path))
        else {
            continue;
        };
        for site in &file.sites {
            if site.span.start > span.byte_start || span.byte_start >= site.span.end {
                continue;
            }
            match site.placed {
                witness::Placed::Witnesses => refused.claims.extend(site.claims.iter().copied()),
                witness::Placed::Marker => refused.markers.extend(site.claims.iter().copied()),
            }
        }
    }
    refused
}

/// What the compiler would not take, by what it costs.
///
/// A diagnostic in a condition's witnesses refuses the claim, because the
/// whole of it rests on the compiler accepting them. One in a body's marker
/// refuses only the marker: the claim stands, with a coverage region as the
/// one premise left to establish it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Refused {
    /// The mutants whose claim the compiler would not take.
    claims: BTreeSet<u32>,
    /// The mutants whose body the compiler would not take a marker in.
    markers: BTreeSet<u32>,
}

/// Whether the path a diagnostic names is the file that was written.
fn ends_with(reported: &str, path: &str) -> bool {
    let reported = reported.replace('\\', "/");
    reported == path || reported.ends_with(&format!("/{path}"))
}
