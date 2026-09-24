// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rewriting a file so that every compilable mutant of it lives in the file at once, dormant behind a guard.

mod guards;
mod observable;
mod runtime;
mod steps;
pub mod witness;

/// The name a generated module of `stem` can take in `text`, dodging every identifier the file spells.
///
/// # Errors
///
/// Returns [`ModuleNameError`] when `text` is not a Rust token stream or the collision suffix namespace cannot be searched without overflow.
pub fn module_named_for(text: &str, stem: &str) -> Result<String, ModuleNameError> {
    runtime::module_named(text, stem)
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::flatten::flatten;
use crate::interval::{self, Item, Node};
use crate::span::Span;
use crate::splice::{Splice, apply, count_lines};
use crate::syntax::branch::Marker;
use crate::syntax::{Form, Found, SiteHint};

pub use runtime::{
    ACTIVE_ENV, CATALOG_ENV, COMPILED_CATALOG_ENV, DELAY_ENV, MODULE_STEM, ModuleNameError,
    RUNTIME_MARKER, Rendering, RuntimeRenderError, STALE_CATALOG_EXIT, STEP_NONCE_ENV,
    STEP_NOTICE_ENV, STEP_NOTICE_SCHEMA, STEP_PROTOCOL_EXIT, STEP_STATE_ENV, STEP_STATE_SCHEMA,
    STEPS_ENV, TOUCH_ENV, TOUCH_UNAVAILABLE_EXIT, module_name, render,
};

/// The first words the runtime prints before it exits [`runtime::STALE_CATALOG_EXIT`].
pub const STALE_CATALOG_MARKER: &str = "rust-mutants: this binary was built from catalog ";

/// The lints the generated module's one allowance names, in the order it names them.
pub const GENERATED_MODULE_ALLOWED_LINTS: [&str; 2] = ["dead_code", "unused_qualifications"];

/// The rustc lint groups that hold every lint the generated module allows.
const GENERATED_MODULE_ALLOWING_GROUPS: [&str; 2] = ["warnings", "unused"];

/// Every name whose `forbid` level makes the generated module's allowance illegal.
pub(crate) const GENERATED_MODULE_CONFLICTING_LINTS: [&str; 4] = conflicting();

/// The allowed lints together with the groups that hold them.
const fn conflicting() -> [&'static str; 4] {
    let [dead_code, unused_qualifications] = GENERATED_MODULE_ALLOWED_LINTS;
    let [warnings, unused] = GENERATED_MODULE_ALLOWING_GROUPS;
    [warnings, unused, dead_code, unused_qualifications]
}

/// The exact, private exception carried by repository-generated support modules.
/// It never decorates user-authored code.
pub const GENERATED_MODULE_ALLOW_ATTRIBUTE: &str = "#[allow(dead_code, unused_qualifications)]";

/// One mutant placed at its rewrite site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The mutant's dense catalog index, which the guard names.
    pub index: u32,
    /// The mutant's full identity.
    pub id: String,
    /// The bytes the edit replaces.
    pub edit: Span,
    /// Exactly the bytes `edit` covers in the pristine file.
    pub original: Vec<u8>,
    /// What those bytes become.
    pub replacement: Vec<u8>,
    /// The rewrite site.
    pub hint: SiteHint,
}

/// One guard the instrumenter placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The mutant's full identity.
    pub id: String,
    /// The form the site took.
    pub form: Form,
    /// The bytes the guard replaced.
    pub site: Span,
}

/// Where one mutant's own text sits in an instrumented file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Branch {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The byte range the alternative occupies in the instrumented text.
    pub span: Span,
}

/// What one file is rewritten with: the mutants, the shape they nest in, and the markers its branch proofs put in it.
#[derive(Debug, Clone, Copy)]
struct Planted<'a> {
    /// Which of them nest inside which, so an outer guard renders the inner ones in its own original branch.
    forest: &'a interval::Forest<Placement>,
    /// The markers that can be written where they are.
    markers: &'a [Marker],
}

/// A rewritten file: its text and where every alternative landed in it.
struct Rewritten {
    text: String,
    branches: Vec<Branch>,
    compared: BTreeSet<u32>,
}

/// A rendered site: its text, where each alternative sits in it, and which of them the guard evaluates beside what it replaces.
struct Rendered {
    text: String,
    branches: Vec<(u32, Span)>,
    compared: BTreeSet<u32>,
}

/// One instrumented file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOutput {
    /// The workspace-relative path.
    pub path: String,
    /// The rewritten text, runtime included.
    pub text: String,
    /// Every guard placed, in catalog order.
    pub guards: Vec<Guard>,
    /// Every alternative branch, in file order: where each mutant's own text landed.
    pub branches: Vec<Branch>,
    /// Every mutant whose guard in this text evaluates its two branches and records whether they differed, ascending.
    pub compared: Vec<u32>,
    /// Every marker this text holds the call for, ascending, which is not every marker it was given: a body inside a guard's own site takes none.
    pub marked: Vec<u32>,
    /// The name the runtime module took.
    pub module: String,
    /// Whether anything was rewritten.
    /// Every mutable file receives control-flow checkpoints, including one with no mutant of its own, so a mutation in another file cannot escape its process-wide step allowance here.
    pub instrumented: bool,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum InstrumentErrorKind {
    /// A candidate is not in the catalog.
    UnknownMutant,
    /// The source is not the one the candidates were discovered from.
    SourceMismatch,
    /// Two rewrite sites partially overlap, which the syntax tree cannot produce: an engine bug rather than a fact about the program.
    SiteConflict,
    /// An alternative could not be folded onto one line.
    FlattenFailed,
    /// The rewrites could not be applied to the file.
    SpliceFailed,
    /// A rewrite would have moved a line, breaking the one invariant every consumer of a position depends on.
    LinesMoved,
    /// A mutant index makes the runtime's inclusive `u32` window unrepresentable.
    IndexReserved,
}

impl InstrumentErrorKind {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::UnknownMutant => error::INSTRUMENT_UNKNOWN_MUTANT,
            Self::SourceMismatch => error::INSTRUMENT_SOURCE_MISMATCH,
            Self::SiteConflict => error::INSTRUMENT_SITE_CONFLICT,
            Self::FlattenFailed => error::INSTRUMENT_FLATTEN_FAILED,
            Self::SpliceFailed => error::INSTRUMENT_SPLICE_FAILED,
            Self::LinesMoved => error::INSTRUMENT_LINES_MOVED,
            Self::IndexReserved => error::INSTRUMENT_INDEX_RESERVED,
        }
    }
}

/// Every error this module returns.
#[derive(Debug)]
pub struct InstrumentError {
    kind: InstrumentErrorKind,
    path: String,
    message: String,
}

impl InstrumentError {
    fn new(kind: InstrumentErrorKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
        }
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> InstrumentErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The file the error is about.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for InstrumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: instrument: {}: {}",
            self.code().code,
            self.path,
            self.message
        )
    }
}

impl std::error::Error for InstrumentError {}

/// Pairs the candidates discovered in one file with their catalog entries.
///
/// # Errors
/// [`InstrumentErrorKind::UnknownMutant`] for a candidate the catalog does not hold, which means the two were computed from different trees.
pub fn plan_file(
    catalog: &Catalog,
    path: &str,
    found: &[Found],
) -> Result<Vec<Placement>, InstrumentError> {
    let mut placements = Vec::new();
    for one in found.iter().filter(|one| one.candidate.path == path) {
        let id = one.candidate.id().map_err(|error| {
            InstrumentError::new(
                InstrumentErrorKind::UnknownMutant,
                path,
                format!("a candidate has no identity: {error}"),
            )
        })?;
        let Some(mutant) = catalog.by_id(id.as_str()) else {
            if catalog
                .duplicates()
                .iter()
                .any(|duplicate| duplicate.dropped_id == id)
            {
                continue;
            }
            return Err(InstrumentError::new(
                InstrumentErrorKind::UnknownMutant,
                path,
                format!(
                    "the catalog does not hold {id}, the {} candidate at byte {}",
                    one.candidate.rule.name, one.candidate.span.start
                ),
            ));
        };
        placements.push(Placement {
            index: mutant.index,
            id: id.to_string(),
            edit: one.candidate.span,
            original: one.candidate.original.clone(),
            replacement: one.candidate.replacement.clone(),
            hint: one.hint.clone(),
        });
    }
    placements.sort_by_key(|placement| placement.index);
    Ok(placements)
}

/// One file to instrument: its bytes, the mutants placed in it, and the markers its branch proofs put in it.
#[derive(Debug, Clone, Copy)]
pub struct Instrumenting<'a> {
    /// The workspace-relative path, which names the file's own runtime module.
    pub path: &'a str,
    /// The pristine bytes.
    pub source: &'a [u8],
    /// The mutants placed in it, each behind a guard.
    pub placements: &'a [Placement],
    /// The markers the branch proofs put at the first statement of the bodies they name.
    pub markers: &'a [Marker],
    /// Every mutant whose guard may compare its two branches, so a run records whether they ever differed.
    pub comparable: &'a BTreeSet<u32>,
    /// Every return replacement whose guard may ask what the value it replaces already held, with the question to ask.
    pub probed: &'a BTreeMap<u32, crate::probe::Question>,
    /// The catalog every guard names, which the runtime refuses to be activated under another of.
    pub catalog_digest: &'a str,
}

/// The pristine source after process-wide checkpoints have been inserted and every catalog position has been mapped into that intermediate source.
struct Checkpointed {
    source: Vec<u8>,
    module: String,
    placements: Vec<Placement>,
    markers: Vec<Marker>,
}

fn guards_of(placements: &[Placement]) -> Vec<Guard> {
    let mut guards: Vec<Guard> = placements
        .iter()
        .map(|placement| Guard {
            index: placement.index,
            id: placement.id.clone(),
            form: placement.hint.form,
            site: placement.hint.site,
        })
        .collect();
    guards.sort_by_key(|guard| guard.index);
    guards
}

fn checkpointed(
    file: &Instrumenting<'_>,
    text: &str,
    module: String,
) -> Result<Checkpointed, InstrumentError> {
    let Instrumenting {
        path,
        source,
        placements,
        markers,
        comparable: _comparable,
        probed: _probed,
        catalog_digest: _catalog_digest,
    } = *file;
    let boundaries = steps::splices(text, &module).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the step checkpoints cannot be placed: {error}"),
        )
    })?;
    let (source, offsets) = apply(source, &boundaries).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            format!("the step checkpoints could not be applied: {error}"),
        )
    })?;
    let mapped_text = std::str::from_utf8(&source).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            format!("the checkpoint rewrite is not valid UTF-8: {error}"),
        )
    })?;
    let placements = placements
        .iter()
        .map(|placement| mapped_placement(placement, mapped_text, &offsets, path))
        .collect::<Result<Vec<_>, _>>()?;
    let markers = markers
        .iter()
        .map(|marker| {
            let (at, exact) = offsets.to_output(marker.at);
            if exact {
                Ok(Marker { at, ..*marker })
            } else {
                Err(InstrumentError::new(
                    InstrumentErrorKind::SiteConflict,
                    path,
                    format!("the marker at byte {} lies inside a checkpoint", marker.at),
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Checkpointed {
        source,
        module,
        placements,
        markers,
    })
}

/// Rewrites one file so that every placed mutant lives in it behind a guard.
///
/// # Errors
/// See [`InstrumentErrorKind`].
pub fn instrument_file(file: &Instrumenting<'_>) -> Result<FileOutput, InstrumentError> {
    let Instrumenting {
        path,
        source,
        placements,
        markers: _original_markers,
        comparable,
        probed,
        catalog_digest,
    } = *file;
    let text = std::str::from_utf8(source).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source is not valid UTF-8: {error}"),
        )
    })?;
    let module = module_name(path, text).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source token stream is invalid: {error}"),
        )
    })?;
    check_pristine(file, text, &module)?;
    let checkpointed = checkpointed(file, text, module)?;
    let bounded_text = std::str::from_utf8(&checkpointed.source).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SpliceFailed,
            path,
            format!("the checkpointed source is no longer valid UTF-8: {error}"),
        )
    })?;
    let worker = File {
        path,
        text: bounded_text,
        module: checkpointed.module,
        comparable,
        probed,
    };
    worker.check_placements(&checkpointed.placements)?;
    let forest = worker.forest(&checkpointed.placements)?;
    let markers = File::markable(&checkpointed.markers, &forest);

    let Rewritten {
        mut text,
        branches,
        compared,
    } = worker.rewrite(
        &checkpointed.source,
        &Planted {
            forest: &forest,
            markers: &markers,
        },
    )?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    worker.append_runtime(
        &mut text,
        &Rendering {
            module: &worker.module,
            catalog_digest,
            placements: &checkpointed.placements,
            markers: &markers,
            newline: worker.newline(),
        },
    )?;
    Ok(FileOutput {
        path: path.to_owned(),
        text,
        guards: guards_of(placements),
        branches,
        compared: compared.into_iter().collect(),
        marked: markers.iter().map(|marker| marker.index).collect(),
        module: worker.module,
        instrumented: true,
    })
}

fn check_pristine(
    file: &Instrumenting<'_>,
    text: &str,
    module: &str,
) -> Result<(), InstrumentError> {
    File {
        path: file.path,
        text,
        module: module.to_owned(),
        comparable: file.comparable,
        probed: file.probed,
    }
    .check_placements(file.placements)
}

fn mapped_placement(
    placement: &Placement,
    source: &str,
    offsets: &crate::splice::OffsetMap,
    path: &str,
) -> Result<Placement, InstrumentError> {
    let map = |span| {
        offsets.map_span(span).map_err(|error| {
            InstrumentError::new(
                InstrumentErrorKind::SiteConflict,
                path,
                format!("a checkpoint cannot preserve source span {span}: {error}"),
            )
        })
    };
    let edit = map(placement.edit)?;
    let site = map(placement.hint.site)?;
    let original = mapped_source(source, edit, "edit", path)?
        .as_bytes()
        .to_vec();
    let site_text = mapped_source(source, site, "site", path)?;
    Ok(Placement {
        index: placement.index,
        id: placement.id.clone(),
        edit,
        original,
        replacement: placement.replacement.clone(),
        hint: SiteHint {
            form: placement.hint.form,
            site,
            site_text: site_text.to_owned(),
            super_depth: placement.hint.super_depth,
        },
    })
}

fn mapped_source<'a>(
    source: &'a str,
    span: Span,
    subject: &str,
    path: &str,
) -> Result<&'a str, InstrumentError> {
    let start = usize::try_from(span.start).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("mapped {subject} {} does not fit this platform", span.start),
        )
    })?;
    let end = usize::try_from(span.end).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("mapped {subject} {} does not fit this platform", span.end),
        )
    })?;
    source.get(start..end).ok_or_else(|| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("mapped {subject} {span} is not in the checkpointed source"),
        )
    })
}

/// One file being rewritten.
struct File<'a> {
    path: &'a str,
    text: &'a str,
    module: String,
    /// Every mutant of this file whose guard may compare its two branches.
    comparable: &'a BTreeSet<u32>,
    /// Every return replacement of this file whose guard may ask what the value it replaces already held.
    probed: &'a BTreeMap<u32, crate::probe::Question>,
}

impl File<'_> {
    /// The line ending the file uses, so the appended runtime matches it.
    fn newline(&self) -> &'static str {
        if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    fn error(&self, kind: InstrumentErrorKind, message: impl Into<String>) -> InstrumentError {
        InstrumentError::new(kind, self.path, message)
    }

    /// Appends the generated runtime after every source rewrite has kept its line boundary intact.
    fn append_runtime(
        &self,
        text: &mut String,
        rendering: &Rendering<'_>,
    ) -> Result<(), InstrumentError> {
        let runtime = render(rendering).map_err(|error| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!("the generated runtime cannot represent this file: {error}"),
            )
        })?;
        text.push_str(&runtime);
        Ok(())
    }

    fn slice(&self, span: Span) -> Result<&str, InstrumentError> {
        let start = usize::try_from(span.start).map_err(|_overflow| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!("{} cannot be represented as a byte offset", span.start),
            )
        })?;
        let end = usize::try_from(span.end).map_err(|_overflow| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!("{} cannot be represented as a byte offset", span.end),
            )
        })?;
        self.text.get(start..end).ok_or_else(|| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!("{span} is not a range of this file"),
            )
        })
    }

    /// Every placement must name bytes this file really holds and an index whose inclusive runtime window is representable.
    fn check_placements(&self, placements: &[Placement]) -> Result<(), InstrumentError> {
        for placement in placements {
            if placement.index == runtime::FIRST_UNREPRESENTABLE_INDEX {
                return Err(self.error(
                    InstrumentErrorKind::IndexReserved,
                    format!(
                        "mutant index {} makes the runtime's inclusive u32 window overflow",
                        placement.index
                    ),
                ));
            }
            let found = self.slice(placement.edit)?.as_bytes();
            if found != placement.original {
                return Err(self.error(
                    InstrumentErrorKind::SourceMismatch,
                    format!(
                        "{} covers {}, but the candidate was taken from {}",
                        placement.edit,
                        crate::telling::LosslessBytes::new(found),
                        crate::telling::LosslessBytes::new(&placement.original)
                    ),
                ));
            }
            let site = placement.hint.site;
            if !site.contains(placement.edit) {
                return Err(self.error(
                    InstrumentErrorKind::SiteConflict,
                    format!(
                        "the edit {} does not lie inside its site {site}",
                        placement.edit
                    ),
                ));
            }
            if self.slice(site)? != placement.hint.site_text {
                return Err(self.error(
                    InstrumentErrorKind::SourceMismatch,
                    format!("the site {site} is not the text the candidate recorded"),
                ));
            }
        }
        Ok(())
    }

    /// Arranges the sites by containment.
    /// Sites come from the syntax tree, so they nest; a partial overlap is an engine bug and is refused.
    fn forest(
        &self,
        placements: &[Placement],
    ) -> Result<interval::Forest<Placement>, InstrumentError> {
        let items = placements
            .iter()
            .map(|placement| Item {
                span: placement.hint.site,
                payload: placement.clone(),
            })
            .collect();
        let (forest, conflicts) = interval::build(items);
        if let Some(conflict) = conflicts.first() {
            return Err(self.error(
                InstrumentErrorKind::SiteConflict,
                format!(
                    "the site of mutant {} ({}) conflicts with {}: {:?}",
                    conflict.item.payload.index,
                    conflict.item.span,
                    conflict.against,
                    conflict.reason
                ),
            ));
        }
        Ok(forest)
    }

    /// Applies every guard to the file's bytes, and reports where each alternative landed in the result.
    /// The markers that can be written where they are, which is every one outside every guard's own site.
    fn markable(markers: &[Marker], forest: &interval::Forest<Placement>) -> Vec<Marker> {
        markers
            .iter()
            .copied()
            .filter(|marker| {
                !forest
                    .roots()
                    .iter()
                    .any(|root| root.span.start < marker.at && marker.at < root.span.end)
            })
            .collect()
    }

    /// The call one marker becomes, in one line.
    fn marker(&self, marker: &Marker) -> Splice {
        let path = guards::named(&self.module, marker.super_depth, "body");
        Splice {
            span: Span {
                start: marker.at,
                end: marker.at,
            },
            original: Vec::new(),
            replacement: format!("{path}({}); ", marker.index).into_bytes(),
        }
    }

    /// Converts a platform string offset into the catalog's on-wire offset.
    fn offset(&self, offset: usize, about: &str) -> Result<u32, InstrumentError> {
        u32::try_from(offset).map_err(|_overflow| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!("{about} byte offset {offset} exceeds the u32 source boundary"),
            )
        })
    }

    /// Moves a rendered span without inventing a representable endpoint on overflow.
    fn shifted(&self, span: Span, by: u32, about: &str) -> Result<Span, InstrumentError> {
        let start = span.start.checked_add(by).ok_or_else(|| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!("{about} start overflowed while shifting {span} by {by}"),
            )
        })?;
        let end = span.end.checked_add(by).ok_or_else(|| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!("{about} end overflowed while shifting {span} by {by}"),
            )
        })?;
        Ok(Span { start, end })
    }

    /// Converts a composed guard's platform offsets and places its nested branches inside the original arm.
    fn composed_branches(
        &self,
        composed: &guards::Composed,
        nested: &[(u32, Span)],
    ) -> Result<Vec<(u32, Span)>, InstrumentError> {
        let capacity = composed
            .alternatives
            .len()
            .checked_add(nested.len())
            .ok_or_else(|| {
                self.error(
                    InstrumentErrorKind::SpliceFailed,
                    "a composed guard has too many branches",
                )
            })?;
        let mut branches = Vec::with_capacity(capacity);
        for (index, range) in &composed.alternatives {
            branches.push((
                *index,
                Span {
                    start: self.offset(range.start, "alternative branch start")?,
                    end: self.offset(range.end, "alternative branch end")?,
                },
            ));
        }
        let original_at = self.offset(composed.original_at, "original branch start")?;
        for (index, span) in nested {
            branches.push((*index, self.shifted(*span, original_at, "nested branch")?));
        }
        Ok(branches)
    }

    fn rewrite(&self, source: &[u8], planted: &Planted<'_>) -> Result<Rewritten, InstrumentError> {
        let Planted { forest, markers } = *planted;
        let mut splices = Vec::new();
        let mut roots = Vec::new();
        let mut compared = BTreeSet::new();
        for root in forest.roots() {
            let rendered = self.render(root)?;
            splices.push(self.splice(root.span, rendered.text.clone())?);
            compared.extend(rendered.compared.iter().copied());
            roots.push((root.span, rendered));
        }
        splices.extend(markers.iter().map(|marker| self.marker(marker)));
        splices.sort_by_key(|splice| splice.span.start);
        let (rewritten, offsets) = apply(source, &splices).map_err(|error| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!("the guards could not be applied: {error}"),
            )
        })?;
        let text = String::from_utf8(rewritten).map_err(|error| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!("the rewrite is not valid UTF-8: {error}"),
            )
        })?;
        let mut branches: Vec<Branch> = Vec::new();
        for (span, rendered) in roots {
            let (at, exact) = offsets.to_output(span.start);
            if !exact {
                return Err(self.error(
                    InstrumentErrorKind::SpliceFailed,
                    format!(
                        "the rewritten root at byte {} has no exact offset",
                        span.start
                    ),
                ));
            }
            for (index, branch) in rendered.branches {
                let start = branch.start.checked_add(at).ok_or_else(|| {
                    self.error(
                        InstrumentErrorKind::SpliceFailed,
                        format!("mutant {index}'s rewritten branch start overflowed"),
                    )
                })?;
                let end = branch.end.checked_add(at).ok_or_else(|| {
                    self.error(
                        InstrumentErrorKind::SpliceFailed,
                        format!("mutant {index}'s rewritten branch end overflowed"),
                    )
                })?;
                branches.push(Branch {
                    index,
                    span: Span { start, end },
                });
            }
        }
        branches.sort_by_key(|branch| (branch.span.start, branch.index));
        Ok(Rewritten {
            text,
            branches,
            compared,
        })
    }

    /// Renders one site: its alternatives, then its original branch with the sites nested inside it already rendered.
    fn render(&self, node: &Node<Placement>) -> Result<Rendered, InstrumentError> {
        let Rendered {
            text: original,
            branches: nested,
            mut compared,
        } = self.original_branch(node)?;

        let site = self.slice(node.span)?;
        let mut alternatives = Vec::with_capacity(node.alternatives.len());
        for placement in &node.alternatives {
            alternatives.push(guards::Alternative {
                index: placement.index,
                text: self.alternative(node.span, site, placement)?,
                comparable: self.comparable.contains(&placement.index),
                probe: self.probed.get(&placement.index).copied(),
            });
        }
        let form = node
            .alternatives
            .first()
            .map_or(Form::E, |placement| placement.hint.form);
        let depth = node
            .alternatives
            .first()
            .map_or(0, |placement| placement.hint.super_depth);
        let composed = guards::compose(
            form,
            &guards::Paths {
                module: &self.module,
                depth,
            },
            &alternatives,
            &original,
        )
        .map_err(|error| {
            self.error(
                InstrumentErrorKind::SpliceFailed,
                format!(
                    "the guard at {} cannot represent its offsets: {error}",
                    node.span
                ),
            )
        })?;
        if count_lines(composed.text.as_bytes()) != count_lines(site.as_bytes()) {
            return Err(self.error(
                InstrumentErrorKind::LinesMoved,
                format!(
                    "the guard at {} does not keep the site's line count",
                    node.span
                ),
            ));
        }
        let branches = self.composed_branches(&composed, &nested)?;
        compared.extend(composed.compared);
        Ok(Rendered {
            text: composed.text,
            branches,
            compared,
        })
    }

    /// The branch that keeps the original: the site's own bytes with every site nested inside it already rendered, and their branch ranges shifted to where they landed.
    fn original_branch(&self, node: &Node<Placement>) -> Result<Rendered, InstrumentError> {
        let bounds = |from: u32, to: u32| {
            Span::new(from, to)
                .map_err(|error| self.error(InstrumentErrorKind::SiteConflict, error.to_string()))
        };
        let mut text = String::new();
        let mut branches: Vec<(u32, Span)> = Vec::new();
        let mut compared = BTreeSet::new();
        let mut cursor = node.span.start;
        for child in &node.children {
            text.push_str(self.slice(bounds(cursor, child.span.start)?)?);
            let rendered = self.render(child)?;
            let at = self.offset(text.len(), "nested guard start")?;
            for (index, span) in rendered.branches {
                branches.push((index, self.shifted(span, at, "nested branch")?));
            }
            compared.extend(rendered.compared);
            text.push_str(&rendered.text);
            cursor = child.span.end;
        }
        text.push_str(self.slice(bounds(cursor, node.span.end)?)?);
        Ok(Rendered {
            text,
            branches,
            compared,
        })
    }

    /// One alternative: the pristine site with exactly this edit applied, folded onto one line.
    fn alternative(
        &self,
        site: Span,
        site_text: &str,
        placement: &Placement,
    ) -> Result<String, InstrumentError> {
        let head = self.slice(
            Span::new(site.start, placement.edit.start).map_err(|error| {
                self.error(InstrumentErrorKind::SiteConflict, error.to_string())
            })?,
        )?;
        let tail =
            self.slice(Span::new(placement.edit.end, site.end).map_err(|error| {
                self.error(InstrumentErrorKind::SiteConflict, error.to_string())
            })?)?;
        let replacement = std::str::from_utf8(&placement.replacement).map_err(|error| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!(
                    "the replacement of mutant {} is not UTF-8: {error}",
                    placement.index
                ),
            )
        })?;
        if placement.hint.form == Form::M {
            return Ok(replacement.to_owned());
        }
        let text = format!("{head}{replacement}{tail}");
        debug_assert!(!site_text.is_empty() || text.is_empty());
        if text.trim().is_empty() {
            return Ok(String::new());
        }
        flatten(&text).map_err(|error| {
            self.error(
                InstrumentErrorKind::FlattenFailed,
                format!(
                    "the {} alternative of mutant {} cannot be folded onto one line: {error}",
                    placement.hint.form, placement.index
                ),
            )
        })
    }

    fn splice(&self, span: Span, replacement: String) -> Result<Splice, InstrumentError> {
        Ok(Splice {
            span,
            original: self.slice(span)?.as_bytes().to_vec(),
            replacement: replacement.into_bytes(),
        })
    }
}
