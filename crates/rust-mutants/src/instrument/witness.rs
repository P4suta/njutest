// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The witness tree: putting to the compiler the one question the syntax cannot answer about a branch proof.
//!
//! A claim rests on the whole condition being inert, and syntax can decide
//! everything about that except the types: `a < b` is a call whenever `a` is
//! not a primitive, and a call may do anything. So each comparison and each
//! cast becomes a statement in front of the condition whose argument types the
//! compiler must accept — a sealed trait implemented for the primitives and for
//! references to them, and nothing else. A claim whose witness the compiler
//! refuses is a claim this release does not make (ADR 0008).
//!
//! The witnesses are written into the pristine tree, checked, and taken out
//! again before anything is instrumented. They add no line, so every position
//! the catalog reports still points where it did.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::span::Span;
use crate::splice::{Splice, apply};
use crate::syntax::branch::{Claim, Witness};

use super::runtime::module_named;
use super::{InstrumentError, InstrumentErrorKind};

/// The module the witness functions live in.
pub const MODULE_STEM: &str = "__rmw";

/// The line a reader will find at the end of a witnessed file.
pub const MARKER: &str = "rust-mutants-witness-v1";

/// One rewrite a witnessed file carries, and every mutant whose claim rests on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// Where the rewrite landed in the rewritten text.
    pub span: Span,
    /// The mutants whose claims this rewrite carries, ascending.
    pub claims: Vec<u32>,
    /// What the rewrite is, which decides what a diagnostic landing in it costs.
    pub placed: Placed,
}

/// What one rewrite in a witnessed file is.
///
/// A diagnostic in a condition's witnesses refuses the claim: the whole of it
/// rests on the compiler accepting them. One in a body's marker refuses only
/// the marker, and the claim stands with a coverage region as its premise —
/// a body a call cannot go into is a body a `const` context holds, not a body
/// the claim was wrong about. One in a probe's binding refuses only the probe:
/// the mutant is still measured, by running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Placed {
    /// The statements a condition's witnesses became.
    Witnesses,
    /// The call at a body's first statement.
    Marker,
    /// The binding a returned value was put to the compiler through.
    Probe,
}

/// One file with its witnesses written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessFile {
    /// The workspace-relative path.
    pub path: String,
    /// The rewritten text, witness module included.
    pub text: String,
    /// Where each condition's witnesses landed.
    pub sites: Vec<Site>,
    /// Whether anything was rewritten. A file with no claim comes back byte for byte.
    pub witnessed: bool,
}

/// One thing to put to the compiler, and where the mutant that carries it sits.
///
/// Two questions share one rewrite. A branch proof needs the condition to be
/// inert *and* names the body it gates; a guard that wants to compare its two
/// branches needs only the condition to be inert. Both are the same witnesses
/// written in front of the same condition, so a site with a body and one
/// without are one entry here with the body optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The whole condition, which is what the witnesses are written in front of.
    pub condition: Span,
    /// The body a branch proof about this edit names, or nothing where the edit supports no proof and only the comparison.
    pub body: Option<Span>,
    /// What the compiler must vouch for.
    pub witnesses: Vec<Witness>,
    /// How many `super::` segments separate the condition's inline module from the file root.
    pub super_depth: u32,
}

impl Claimed {
    /// The same thing as a claim, when it names a body.
    #[must_use]
    pub fn claim(&self) -> Option<Claim> {
        Some(Claim {
            condition: self.condition,
            body: self.body?,
            witnesses: self.witnesses.clone(),
        })
    }
}

/// What one file puts to the compiler.
///
/// Two questions of different shapes travel together because one `cargo check`
/// answers both. A condition is asked whether it is inert; a returned value is
/// asked whether its type is one a guard may compare against what a return
/// replacement would write. Neither can overlap the other: a probeable
/// expression holds no `if` and no `while`, so no condition of one sits inside
/// a probe's own bytes.
#[derive(Debug, Clone, Copy)]
pub struct Asking<'a> {
    /// Every condition to witness, with the mutants resting on it.
    pub conditions: &'a [Claimed],
    /// Every returned value whose type a probe rests on.
    pub probes: &'a [Probing],
}

impl Asking<'_> {
    /// Whether this file has anything to put to the compiler at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.probes.is_empty()
    }
}

/// One returned value whose type decides whether a guard may probe the mutation that replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probing {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The returned expression, which is what the guard replaces and what the binding is put to.
    pub value: Span,
    /// What the probe asks about it.
    pub question: crate::probe::form::Question,
    /// How many `super::` segments separate the value's inline module from the file root.
    pub super_depth: u32,
}

/// The name a probe's binding takes, which a program is unlikely to spell and which shadows rather than collides where one does.
const BINDING: &str = "__rmw_value";

/// Writes every claim's witnesses into `source`.
///
/// # Errors
/// [`InstrumentErrorKind::LinesMoved`] when a rewrite would move a line, which
/// no witness may do, and the splice's own refusals.
pub fn witness_file(
    path: &str,
    source: &[u8],
    asking: &Asking<'_>,
) -> Result<WitnessFile, InstrumentError> {
    let claims = asking.conditions;
    let text = String::from_utf8_lossy(source).into_owned();
    if asking.is_empty() {
        return Ok(WitnessFile {
            path: path.to_owned(),
            text,
            sites: Vec::new(),
            witnessed: false,
        });
    }
    let module = module_named(&text, MODULE_STEM);
    let mut conditions: BTreeMap<Span, Condition> = BTreeMap::new();
    for claimed in claims {
        let entry = conditions
            .entry(claimed.condition)
            .or_insert_with(|| Condition {
                indices: Vec::new(),
                bodies: BTreeMap::new(),
                witnesses: claimed.witnesses.clone(),
                depth: claimed.super_depth,
            });
        entry.indices.push(claimed.index);
        if let Some(body) = claimed.body {
            entry.bodies.entry(body).or_default().push(claimed.index);
        }
    }

    let (splices, owners) = plan(
        Reading {
            path,
            source,
            text: &text,
        },
        &conditions,
        asking.probes,
        &module,
    )?;
    let placed = owners;
    let (bytes, map) = apply(source, &splices).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path.to_owned(),
            error.to_string(),
        )
    })?;
    let mut rewritten = String::from_utf8_lossy(&bytes).into_owned();
    if crate::splice::count_lines(&bytes) != crate::splice::count_lines(source) {
        return Err(InstrumentError::new(
            InstrumentErrorKind::LinesMoved,
            path.to_owned(),
            "a witness moved a line, which every position in the catalog depends on not \
             happening"
                .to_owned(),
        ));
    }
    let sites = splices
        .iter()
        .zip(placed)
        .map(|(one, (claims, placed))| Site {
            span: landed(&map, one),
            claims,
            placed,
        })
        .collect();
    if !rewritten.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten.push_str(&runtime(&module));
    Ok(WitnessFile {
        path: path.to_owned(),
        text: rewritten,
        sites,
        witnessed: true,
    })
}

/// The statements one condition's witnesses become, in one line.
fn statements(witnesses: &[Witness], text: &str, module: &str, depth: u32) -> String {
    let mut out = String::new();
    for witness in witnesses {
        let arguments: Vec<String> = witness
            .operands
            .iter()
            .filter_map(|span| text.get(at(span.start)..at(span.end)))
            .map(|operand| format!("&({})", one_line(operand)))
            .collect();
        if arguments.len() != witness.operands.len() {
            continue;
        }
        let written = write!(
            out,
            "{}{module}::{}({}); ",
            "super::".repeat(usize::try_from(depth).unwrap_or(0)),
            witness.kind.function(),
            arguments.join(", ")
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    out
}

/// What one rewrite carries: the mutants whose claims rest on it, and what it is.
type Owned = (Vec<u32>, Placed);

/// One condition to witness, with every mutant that rests on it and every body they name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Condition {
    /// Every mutant whose edit sits in this condition.
    indices: Vec<u32>,
    /// Each body a branch proof about one of them names, with the mutants that name it.
    bodies: BTreeMap<Span, Vec<u32>>,
    /// What the compiler must vouch for.
    witnesses: Vec<Witness>,
    /// How many `super::` segments separate the condition's inline module from the file root.
    depth: u32,
}

/// One file as it is and as text.
#[derive(Debug, Clone, Copy)]
struct Reading<'a> {
    path: &'a str,
    source: &'a [u8],
    text: &'a str,
}

/// What to write where, and which claims each rewrite carries.
fn plan(
    file: Reading<'_>,
    conditions: &BTreeMap<Span, Condition>,
    probes: &[Probing],
    module: &str,
) -> Result<(Vec<Splice>, Vec<Owned>), InstrumentError> {
    let Reading { path, source, text } = file;
    let mut splices = Vec::new();
    let mut owners = Vec::new();
    let mut bodies: BTreeMap<Span, (Vec<u32>, u32)> = BTreeMap::new();
    for condition in conditions.values() {
        for (body, indices) in &condition.bodies {
            let entry = bodies
                .entry(*body)
                .or_insert_with(|| (Vec::new(), condition.depth));
            entry.0.extend(indices.iter().copied());
        }
    }
    for (body, (indices, depth)) in &bodies {
        let mut claims = indices.clone();
        claims.sort_unstable();
        claims.dedup();
        let Some(index) = claims.first().copied() else {
            continue;
        };
        let after = body.start.saturating_add(1);
        if source.get(at(body.start)..at(after)) != Some(b"{".as_slice()) {
            continue;
        }
        splices.push(Splice {
            span: Span {
                start: after,
                end: after,
            },
            original: Vec::new(),
            replacement: marker(module, *depth, index).into_bytes(),
        });
        owners.push((claims, Placed::Marker));
    }
    for (condition, held) in conditions {
        let (indices, depth) = (&held.indices, &held.depth);
        let original = source
            .get(at(condition.start)..at(condition.end))
            .ok_or_else(|| {
                InstrumentError::new(
                    InstrumentErrorKind::SourceMismatch,
                    path.to_owned(),
                    format!("the condition at {condition} is not inside the source"),
                )
            })?;
        let statements = statements(&held.witnesses, text, module, *depth);
        let mut replacement = format!("({{ {statements}").into_bytes();
        replacement.extend_from_slice(original);
        replacement.extend_from_slice(b" })");
        splices.push(Splice {
            span: *condition,
            original: original.to_vec(),
            replacement,
        });
        let mut claims = indices.clone();
        claims.sort_unstable();
        claims.dedup();
        owners.push((claims, Placed::Witnesses));
    }
    probed(
        &Reading { path, source, text },
        probes,
        module,
        (&mut splices, &mut owners),
    )?;
    Ok((splices, owners))
}

/// Binds each probed value so that the compiler is asked what type it is, in the shape the guard will hold.
///
/// The value is kept verbatim and only what surrounds it is written, so a
/// value spelled over four lines still occupies four. Nothing here can overlap
/// a condition's rewrite: a value a probe is offered for holds no `if` and no
/// `while`, so no condition of one sits inside its bytes.
fn probed(
    file: &Reading<'_>,
    probes: &[Probing],
    module: &str,
    (splices, owners): (&mut Vec<Splice>, &mut Vec<Owned>),
) -> Result<(), InstrumentError> {
    let Reading { path, source, .. } = *file;
    for probe in probes {
        let original = source
            .get(at(probe.value.start)..at(probe.value.end))
            .ok_or_else(|| {
                InstrumentError::new(
                    InstrumentErrorKind::SourceMismatch,
                    path.to_owned(),
                    format!("the value at {} is not inside the source", probe.value),
                )
            })?;
        let mut replacement = format!("({{ let {BINDING} = ").into_bytes();
        replacement.extend_from_slice(original);
        replacement.extend_from_slice(
            format!(
                "; {}{module}::{}(&{BINDING}); {BINDING} }})",
                "super::".repeat(usize::try_from(probe.super_depth).unwrap_or(0)),
                probe.question.witness(),
            )
            .as_bytes(),
        );
        splices.push(Splice {
            span: probe.value,
            original: original.to_vec(),
            replacement,
        });
        owners.push((vec![probe.index], Placed::Probe));
    }
    Ok(())
}

/// Where one rewrite ended up in the rewritten text.
///
/// An insertion leaves nothing of the source at its offset, so the offset maps
/// to just past what was written rather than to the start of it; a
/// replacement maps to where its own bytes begin.
fn landed(map: &crate::splice::OffsetMap, splice: &Splice) -> Span {
    let written = u32::try_from(splice.replacement.len()).unwrap_or(u32::MAX);
    let at = map.to_output(splice.span.start).0;
    let start = if splice.span.start == splice.span.end {
        at.saturating_sub(written)
    } else {
        at
    };
    Span {
        start,
        end: start.saturating_add(written),
    }
}

/// The call written at a body's first statement, in one line.
#[must_use]
pub fn marker(module: &str, depth: u32, index: u32) -> String {
    format!(
        "{}{module}::body({index}); ",
        "super::".repeat(usize::try_from(depth).unwrap_or(0))
    )
}

/// One offset as an index. Every offset here came from a `u32` span of a file this process read, and a file larger than a `usize` cannot have been read at all.
fn at(offset: u32) -> usize {
    usize::try_from(offset).unwrap_or(usize::MAX)
}

/// The operand with its newlines turned to spaces, so a witness never moves a line.
fn one_line(operand: &str) -> String {
    operand.replace(['\n', '\r'], " ")
}

/// The module the witness statements call into.
fn runtime(module: &str) -> String {
    let impls = IMPLS
        .replace(
            "{{OBSERVABLE}}",
            &super::observable::declaration(OBSERVABLE, "__rmw_std"),
        )
        .replace(
            "{{PROBE_BOUND}}",
            &super::observable::bound(OBSERVABLE, "__rmw_std"),
        );
    format!(
        "#[doc(hidden)] {allow}mod {module} {{ // {MARKER} — generated; DO NOT EDIT\n\
         {impls}\n\
         }}\n",
        allow = super::ALLOW_ATTRIBUTE,
    )
}

/// The sealed traits and the two functions. `W` names the types whose comparison the standard library defines, and nothing else, which is exactly the question the syntax could not answer.
///
/// `str`, a slice, and the owned types beside them are in it for the same
/// reason the primitives are: comparing two of them runs none of the program's
/// code. The comparison is the library's, it cannot panic, it allocates
/// nothing, and it terminates — which is the whole of what a claim needs. A
/// container is in it only when what it holds is, because a `Vec<T>`
/// comparison is `T`'s comparison in a loop.
///
/// The two operands are asked about **separately**, which is what lets a
/// `String` be compared with a `&str`. It is no weaker for it: a comparison
/// between two of these types can only be the standard library's, because
/// coherence lets nobody add a `PartialEq` or `PartialOrd` impl between two
/// types they own neither of. Bring a type of your own to either side and it
/// is not in `W`, whichever side it is on.
///
/// `std` is linked under a name of this module's own, as [ADR 0011] has the
/// runtime do. A crate the host cannot lend `std` to is one this engine skips
/// whole, so no witness is ever written into one.
///
/// [ADR 0011]: ../../../../docs/adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md
const IMPLS: &str = "\
    extern crate std as __rmw_std;
    pub(crate) trait W {}
    pub(crate) trait P {}
    impl W for i8 {} impl W for i16 {} impl W for i32 {} impl W for i64 {} impl W for i128 {}
    impl W for isize {} impl W for u8 {} impl W for u16 {} impl W for u32 {} impl W for u64 {}
    impl W for u128 {} impl W for usize {} impl W for f32 {} impl W for f64 {}
    impl W for bool {} impl W for char {} impl W for () {}
    impl W for str {} impl W for __rmw_std::string::String {}
    impl W for __rmw_std::ffi::OsStr {} impl W for __rmw_std::ffi::OsString {}
    impl W for __rmw_std::path::Path {} impl W for __rmw_std::path::PathBuf {}
    impl<T: W> W for [T] {}
    impl<T: W, const N: usize> W for [T; N] {}
    impl<T: W> W for __rmw_std::vec::Vec<T> {}
    impl<T: W> W for __rmw_std::option::Option<T> {}
    impl<T: W + ?Sized> W for &T {}
    impl<T: W + ?Sized> W for &mut T {}
    impl P for i8 {} impl P for i16 {} impl P for i32 {} impl P for i64 {} impl P for i128 {}
    impl P for isize {} impl P for u8 {} impl P for u16 {} impl P for u32 {} impl P for u64 {}
    impl P for u128 {} impl P for usize {} impl P for f32 {} impl P for f64 {}
    impl P for bool {} impl P for char {}
    impl<T: P + ?Sized> P for &T {}
    impl<T: P + ?Sized> P for &mut T {}
    #[inline(always)] pub(crate) fn w_ord<A: W + ?Sized, B: W + ?Sized>(_a: &A, _b: &B) {}
    #[inline(always)] pub(crate) fn w_prim<T: P + ?Sized>(_x: &T) {}
    #[inline(always)] pub(crate) fn body(_k: u32) {}
    {{OBSERVABLE}}
    #[inline(always)] pub(crate) fn w_default<T: {{PROBE_BOUND}}>(_x: &T) {}
    #[inline(always)] pub(crate) fn w_true(_x: &bool) {}
    #[inline(always)] pub(crate) fn w_ok_default<T: {{PROBE_BOUND}}, E>(_x: &__rmw_std::result::Result<T, E>) {}
    #[inline(always)] pub(crate) fn w_some_default<T: {{PROBE_BOUND}}>(_x: &__rmw_std::option::Option<T>) {}";

/// The trait the witness tree names the types a probe may compare a value of.
const OBSERVABLE: &str = "O";
