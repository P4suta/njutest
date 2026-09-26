// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The witness tree: putting to the compiler the one question the syntax cannot answer about a branch proof.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::span::Span;
use crate::splice::{Splice, apply};
use crate::syntax::branch::{Claim, Witness};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Whether anything was rewritten.
    /// A file with no claim comes back byte for byte.
    pub witnessed: bool,
}

/// One thing to put to the compiler, and where the mutant that carries it sits.
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
    pub question: crate::probe::Question,
    /// How many `super::` segments separate the value's inline module from the file root.
    pub super_depth: u32,
}

/// The name a probe's binding takes, which a program is unlikely to spell and which shadows rather than collides where one does.
const BINDING: &str = "__rmw_value";

/// Writes every claim's witnesses into `source`.
///
/// # Errors
/// [`InstrumentErrorKind::LinesMoved`] when a rewrite would move a line, which no witness may do, and the splice's own refusals.
pub fn witness_file(
    path: &str,
    source: &[u8],
    asking: &Asking<'_>,
) -> Result<WitnessFile, InstrumentError> {
    let claims = asking.conditions;
    let text = std::str::from_utf8(source)
        .map_err(|error| {
            InstrumentError::new(
                InstrumentErrorKind::SourceMismatch,
                path,
                format!("the source is not valid UTF-8: {error}"),
            )
        })?
        .to_owned();
    if asking.is_empty() {
        return Ok(WitnessFile {
            path: path.to_owned(),
            text,
            sites: Vec::new(),
            witnessed: false,
        });
    }
    let module = super::module_named_for(&text, MODULE_STEM).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source token stream is invalid: {error}"),
        )
    })?;
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

    let plan = plan(
        Reading {
            path,
            source,
            text: &text,
        },
        &conditions,
        asking.probes,
        &module,
    )?;
    apply_plan(path, source, &module, plan)
}

/// Applies one validated witness plan and records the exact output spans.
fn apply_plan(
    path: &str,
    source: &[u8],
    module: &str,
    plan: Plan,
) -> Result<WitnessFile, InstrumentError> {
    let Plan { splices, placed } = plan;
    let (bytes, map) = apply(source, &splices).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path.to_owned(),
            error.to_string(),
        )
    })?;
    if crate::splice::count_lines(&bytes) != crate::splice::count_lines(source) {
        return Err(InstrumentError::new(
            InstrumentErrorKind::LinesMoved,
            path.to_owned(),
            "a witness moved a line, which every position in the catalog depends on not \
             happening"
                .to_owned(),
        ));
    }
    let mut rewritten = String::from_utf8(bytes).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            format!("the witnessed source is not valid UTF-8: {error}"),
        )
    })?;
    let mut sites = Vec::with_capacity(splices.len());
    for (one, (claims, placed)) in splices.iter().zip(placed) {
        sites.push(Site {
            span: landed(path, &map, one)?,
            claims,
            placed,
        });
    }
    if !rewritten.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten.push_str(&runtime(module));
    Ok(WitnessFile {
        path: path.to_owned(),
        text: rewritten,
        sites,
        witnessed: true,
    })
}

/// The statements one condition's witnesses become, in one line.
fn statements(
    file: &Reading<'_>,
    witnesses: &[Witness],
    module: &str,
    depth: u32,
) -> Result<String, InstrumentError> {
    let mut out = String::new();
    for witness in witnesses {
        let mut arguments = Vec::with_capacity(witness.operands.len());
        for span in &witness.operands {
            let operand = source_text(file.path, file.text, *span, "witness operand")?;
            arguments.push(format!("&({})", one_line(operand)));
        }
        let function = qualified(module, depth, witness.kind.function());
        let written = write!(out, "{function}({}); ", arguments.join(", "));
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    Ok(out)
}

/// What one rewrite carries: the mutants whose claims rest on it, and what it is.
type Owned = (Vec<u32>, Placed);

/// One complete witness rewrite plan.
/// Ownership stays paired with the splice whose output span it describes.
struct Plan {
    splices: Vec<Splice>,
    placed: Vec<Owned>,
}

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
) -> Result<Plan, InstrumentError> {
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
        let Some(index) = indices.iter().copied().min() else {
            continue;
        };
        let after = body.start.checked_add(1).ok_or_else(|| {
            InstrumentError::new(
                InstrumentErrorKind::SourceMismatch,
                path,
                format!("the body at {body} has no representable first byte"),
            )
        })?;
        let opening = source_bytes(
            path,
            source,
            Span {
                start: body.start,
                end: after,
            },
            "body opening",
        )?;
        if opening != b"{" {
            return Err(InstrumentError::new(
                InstrumentErrorKind::SourceMismatch,
                path,
                format!("the body at {body} does not begin with `{{`"),
            ));
        }
        splices.push(Splice {
            span: Span {
                start: after,
                end: after,
            },
            original: Vec::new(),
            replacement: marker(module, *depth, index).into_bytes(),
        });
        owners.push((indices.clone(), Placed::Marker));
    }
    for (condition, held) in conditions {
        let (indices, depth) = (&held.indices, &held.depth);
        let original = source_bytes(path, source, *condition, "condition")?;
        let statements = statements(&file, &held.witnesses, module, *depth)?;
        let mut replacement = format!("({{ {statements}").into_bytes();
        replacement.extend_from_slice(original);
        replacement.extend_from_slice(b" })");
        splices.push(Splice {
            span: *condition,
            original: original.to_vec(),
            replacement,
        });
        owners.push(((*indices).clone(), Placed::Witnesses));
    }
    probed(
        &Reading { path, source, text },
        probes,
        module,
        (&mut splices, &mut owners),
    )?;
    Ok(Plan {
        splices,
        placed: owners,
    })
}

/// Binds each probed value so that the compiler is asked what type it is, in the shape the guard will hold.
fn probed(
    file: &Reading<'_>,
    probes: &[Probing],
    module: &str,
    (splices, owners): (&mut Vec<Splice>, &mut Vec<Owned>),
) -> Result<(), InstrumentError> {
    let Reading { path, source, .. } = *file;
    let mut values: BTreeMap<Span, Vec<&Probing>> = BTreeMap::new();
    for probe in probes {
        values.entry(probe.value).or_default().push(probe);
    }
    for (value, asked) in values {
        let original = source_bytes(path, source, value, "probed value")?;
        let mut questions = String::new();
        for probe in &asked {
            let function = qualified(module, probe.super_depth, probe.question.witness());
            let written = write!(questions, "; {function}(&{BINDING})");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        let mut replacement = format!("({{ let {BINDING} = ").into_bytes();
        replacement.extend_from_slice(original);
        replacement.extend_from_slice(format!("{questions}; {BINDING} }})").as_bytes());
        splices.push(Splice {
            span: value,
            original: original.to_vec(),
            replacement,
        });
        owners.push((
            asked.iter().map(|probe| probe.index).collect(),
            Placed::Probe,
        ));
    }
    Ok(())
}

/// Where one rewrite ended up in the rewritten text.
fn landed(
    path: &str,
    map: &crate::splice::OffsetMap,
    splice: &Splice,
) -> Result<Span, InstrumentError> {
    let written = u32::try_from(splice.replacement.len()).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            "a witness replacement exceeds the u32 source boundary",
        )
    })?;
    let (at, exact) = map.to_output(splice.span.start);
    if !exact {
        return Err(InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            format!(
                "the witness at byte {} has no exact output offset",
                splice.span.start
            ),
        ));
    }
    let start = if splice.span.start == splice.span.end {
        at.checked_sub(written).ok_or_else(|| {
            InstrumentError::new(
                InstrumentErrorKind::SpliceFailed,
                path,
                "an inserted witness begins before the rewritten source",
            )
        })?
    } else {
        at
    };
    let end = start.checked_add(written).ok_or_else(|| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            "a witnessed span exceeds the u32 source boundary",
        )
    })?;
    Ok(Span { start, end })
}

/// The call written at a body's first statement, in one line.
#[must_use]
pub fn marker(module: &str, depth: u32, index: u32) -> String {
    let function = qualified(module, depth, "body");
    format!("{function}({index}); ")
}

/// One runtime function path from an inline-module depth.
fn qualified(module: &str, depth: u32, function: &str) -> String {
    let mut path = String::new();
    for _ in 0..depth {
        path.push_str("super::");
    }
    path.push_str(module);
    path.push_str("::");
    path.push_str(function);
    path
}

/// Reads an exact byte span, refusing platforms on which the catalog offset cannot index memory and refusing stale spans.
fn source_bytes<'a>(
    path: &str,
    source: &'a [u8],
    span: Span,
    about: &str,
) -> Result<&'a [u8], InstrumentError> {
    let start = usize::try_from(span.start).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("{about} start {} does not fit this platform", span.start),
        )
    })?;
    let end = usize::try_from(span.end).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("{about} end {} does not fit this platform", span.end),
        )
    })?;
    source.get(start..end).ok_or_else(|| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("{about} at {span} is not inside the source"),
        )
    })
}

/// Reads an exact UTF-8 source span.
fn source_text<'a>(
    path: &str,
    text: &'a str,
    span: Span,
    about: &str,
) -> Result<&'a str, InstrumentError> {
    let bytes = source_bytes(path, text.as_bytes(), span, about)?;
    std::str::from_utf8(bytes).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("{about} at {span} is not a UTF-8 boundary: {error}"),
        )
    })
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
        "#[doc(hidden)] {allow} mod {module} {{ // {MARKER} — generated; DO NOT EDIT\n\
         {impls}\n\
         }}\n",
        allow = super::GENERATED_MODULE_ALLOW_ATTRIBUTE,
    )
}

/// The sealed traits and the two functions.
/// `W` names the types whose comparison the standard library defines, and nothing else, which is exactly the question the syntax could not answer.
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
