// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rewriting a file so that every compilable mutant of it lives in the file
//! at once, dormant behind a guard.
//!
//! One build then serves every mutant, and activating one costs an
//! environment variable per test process rather than a rebuild. With no
//! variable set, what runs is the original bytes: the instrumented baseline
//! is the program the user wrote.
//!
//! # The three guard forms
//!
//! Which form a site takes is decided by [`crate::syntax`], not here.
//!
//! **Form C**, a boolean position:
//!
//! ```text
//! (__rm::active(3) && (a >= b) || !(__rm::active(3)) && (a > b))
//! ```
//!
//! No block, and so no temporary scope: a lock guard held by the original
//! condition lives exactly as long as it did. The parentheses are load
//! bearing, since a nested Form C site sits inside its parent's `&&` chain.
//!
//! **Form E**, any value position:
//!
//! ```text
//! (if __rm::active(5) { a - b } else { a + b })
//! ```
//!
//! Both branches unify to one type, so `Default::default()` is inferred
//! from the original rather than spelled.
//!
//! **Form S**, a statement:
//!
//! ```text
//! if __rm::active(7) { x -= step; } else { x += step; }
//! ```
//!
//! A deletion is the degenerate branch and not a special case:
//! `if __rm::active(4) { } else { … }` is exactly "this statement does not
//! run".
//!
//! Several mutants of one site are alternatives of one chain rather than
//! nested guards, because mutants are mutually exclusive. Genuinely nested
//! sites become nested guards, composed children first. Every alternative is
//! rendered from the pristine site with that one edit applied and nothing
//! else — a mutant is one edit to the program the user wrote — so only the
//! branch that keeps the original carries the guards of the sites inside it.
//!
//! # Lines are preserved
//!
//! Each alternative is flattened onto one line and the original branch keeps
//! its bytes verbatim, so a guard holds exactly as many line breaks as the
//! bytes it replaced. Every byte of the file therefore stays on the line it
//! started on, which is what lets coverage regions, rustc diagnostics, and
//! mutant positions agree with the pristine file. The runtime module is
//! appended after the last line, where it shifts nothing.
//!
//! # Whether the guard compiles is the validation phase's problem
//!
//! Nothing here type-checks anything, and a mutated copy can still be a
//! program the compiler refuses. Deciding that here would mean type-checking
//! every file to ask a question the compiler is about to answer for free.
//! Instrumentation is a byte rewrite that always produces the same bytes for
//! the same input; whether those bytes compile is established by compiling
//! them.

mod guards;
mod runtime;

use std::collections::BTreeSet;
use std::fmt;

use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::flatten::flatten;
use crate::interval::{self, Item, Node};
use crate::span::Span;
use crate::splice::{Splice, apply, count_lines};
use crate::syntax::{Form, Found, SiteHint};

pub use runtime::{
    ACTIVE_ENV, CATALOG_ENV, MODULE_STEM, RUNTIME_MARKER, STALE_CATALOG_EXIT, module_name,
};

/// The text inserted before the innermost function holding a guard, so that
/// a guard's own lint noise never trips a crate's deny policy anywhere else.
/// It holds no line break.
pub const ALLOW_ATTRIBUTE: &str = "#[allow(warnings)] ";

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
///
/// This is what makes a compiler diagnostic attributable: an error whose
/// span falls inside a branch belongs to exactly that mutant, and one that
/// falls outside every branch is about the program the user wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Branch {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The byte range the alternative occupies in the instrumented text.
    pub span: Span,
}

/// Moves a range along by `by` bytes.
const fn shift(span: Span, by: u32) -> Span {
    Span {
        start: span.start.saturating_add(by),
        end: span.end.saturating_add(by),
    }
}

/// A rewritten file: its text and where every alternative landed in it.
struct Rewritten {
    text: String,
    branches: Vec<Branch>,
}

/// A rendered site: its text and where each alternative sits in it.
struct Rendered {
    text: String,
    branches: Vec<(u32, Span)>,
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
    /// Every alternative branch, in file order: where each mutant's own
    /// text landed.
    pub branches: Vec<Branch>,
    /// The name the runtime module took, empty when none was generated.
    pub module: String,
    /// Whether anything was rewritten. A file with no mutants comes back
    /// byte for byte, without a runtime: an unused module would only be
    /// noise, and a file cargo did not have to recompile is one this run
    /// does not pay for.
    pub instrumented: bool,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentErrorKind {
    /// A candidate is not in the catalog.
    UnknownMutant,
    /// The source is not the one the candidates were discovered from.
    SourceMismatch,
    /// Two rewrite sites partially overlap, which the syntax tree cannot
    /// produce: an engine bug rather than a fact about the program.
    SiteConflict,
    /// An alternative could not be folded onto one line.
    FlattenFailed,
    /// The rewrites could not be applied to the file.
    SpliceFailed,
    /// A rewrite would have moved a line, breaking the one invariant every
    /// consumer of a position depends on.
    LinesMoved,
    /// A mutant index collides with the runtime's sentinel values.
    IndexReserved,
}

impl InstrumentErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 7] = [
        Self::UnknownMutant,
        Self::SourceMismatch,
        Self::SiteConflict,
        Self::FlattenFailed,
        Self::SpliceFailed,
        Self::LinesMoved,
        Self::IndexReserved,
    ];

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
///
/// [`InstrumentErrorKind::UnknownMutant`] for a candidate the catalog does
/// not hold, which means the two were computed from different trees.
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
        let Some(mutant) = catalog.by_id(&id) else {
            // A candidate the catalog deduplicated is not missing: the
            // mutant that won spells the same edit over the same bytes and
            // is placed already, so this one has nothing left to place.
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
            id,
            edit: one.candidate.span,
            original: one.candidate.original.clone(),
            replacement: one.candidate.replacement.clone(),
            hint: one.hint.clone(),
        });
    }
    placements.sort_by_key(|placement| placement.index);
    Ok(placements)
}

/// Rewrites one file so that every placed mutant lives in it behind a guard.
///
/// `catalog_digest` is what the generated runtime checks the activating
/// process against, so that a stale catalog ends the process instead of
/// quietly activating nothing.
///
/// # Errors
///
/// See [`InstrumentErrorKind`].
pub fn instrument_file(
    path: &str,
    source: &[u8],
    placements: &[Placement],
    catalog_digest: &str,
) -> Result<FileOutput, InstrumentError> {
    let text = std::str::from_utf8(source).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source is not valid UTF-8: {error}"),
        )
    })?;
    if placements.is_empty() {
        return Ok(FileOutput {
            path: path.to_owned(),
            text: text.to_owned(),
            guards: Vec::new(),
            branches: Vec::new(),
            module: String::new(),
            instrumented: false,
        });
    }
    let file = File {
        path,
        text,
        module: module_name(text),
    };
    file.check_placements(placements)?;
    let forest = file.forest(placements)?;

    let Rewritten { mut text, branches } = file.rewrite(source, placements, &forest)?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&runtime::render(
        &file.module,
        catalog_digest,
        placements,
        file.newline(),
    ));

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
    Ok(FileOutput {
        path: path.to_owned(),
        text,
        guards,
        branches,
        module: file.module,
        instrumented: true,
    })
}

/// One file being rewritten.
struct File<'a> {
    path: &'a str,
    text: &'a str,
    module: String,
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

    fn slice(&self, span: Span) -> Result<&str, InstrumentError> {
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(usize::MAX);
        self.text.get(start..end).ok_or_else(|| {
            self.error(
                InstrumentErrorKind::SourceMismatch,
                format!("{span} is not a range of this file"),
            )
        })
    }

    /// Every placement must name bytes this file really holds, and an index
    /// the runtime can tell apart from its sentinels.
    fn check_placements(&self, placements: &[Placement]) -> Result<(), InstrumentError> {
        for placement in placements {
            if placement.index >= runtime::LOWEST_SENTINEL {
                return Err(self.error(
                    InstrumentErrorKind::IndexReserved,
                    format!(
                        "mutant index {} collides with the runtime's sentinels",
                        placement.index
                    ),
                ));
            }
            let found = self.slice(placement.edit)?.as_bytes();
            if found != placement.original {
                return Err(self.error(
                    InstrumentErrorKind::SourceMismatch,
                    format!(
                        "{} covers {:?}, but the candidate was taken from {:?}",
                        placement.edit,
                        String::from_utf8_lossy(found),
                        String::from_utf8_lossy(&placement.original)
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

    /// Arranges the sites by containment. Sites come from the syntax tree,
    /// so they nest; a partial overlap is an engine bug and is refused.
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

    /// Applies every guard and every allow attribute to the file's bytes,
    /// and reports where each alternative landed in the result.
    fn rewrite(
        &self,
        source: &[u8],
        placements: &[Placement],
        forest: &interval::Forest<Placement>,
    ) -> Result<Rewritten, InstrumentError> {
        let mut splices = Vec::new();
        let mut roots = Vec::new();
        for root in forest.roots() {
            let rendered = self.render(root)?;
            splices.push(self.splice(root.span, rendered.text.clone())?);
            roots.push((root.span, rendered));
        }
        splices.extend(Self::allow_splices(placements, forest.roots()));
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
            let (at, _exact) = offsets.to_output(span.start);
            branches.extend(rendered.branches.into_iter().map(|(index, branch)| Branch {
                index,
                span: Span {
                    start: branch.start.saturating_add(at),
                    end: branch.end.saturating_add(at),
                },
            }));
        }
        branches.sort_by_key(|branch| (branch.span.start, branch.index));
        Ok(Rewritten { text, branches })
    }

    /// Renders one site: its alternatives, then its original branch with the
    /// sites nested inside it already rendered.
    ///
    /// The branch ranges it reports are relative to the start of the text,
    /// and every nested site's ranges are shifted into it, so a caller that
    /// knows where the text lands knows where every alternative lands.
    fn render(&self, node: &Node<Placement>) -> Result<Rendered, InstrumentError> {
        let Rendered {
            text: original,
            branches: nested,
        } = self.original_branch(node)?;

        let site = self.slice(node.span)?;
        let mut alternatives = Vec::with_capacity(node.alternatives.len());
        for placement in &node.alternatives {
            alternatives.push((
                placement.index,
                self.alternative(node.span, site, placement)?,
            ));
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
            &guards::path(&self.module, depth),
            &alternatives,
            &original,
        );
        if count_lines(composed.text.as_bytes()) != count_lines(site.as_bytes()) {
            return Err(self.error(
                InstrumentErrorKind::LinesMoved,
                format!(
                    "the guard at {} does not keep the site's line count",
                    node.span
                ),
            ));
        }
        let mut branches: Vec<(u32, Span)> = composed
            .alternatives
            .iter()
            .map(|(index, range)| {
                (
                    *index,
                    Span {
                        start: u32::try_from(range.start).unwrap_or(u32::MAX),
                        end: u32::try_from(range.end).unwrap_or(u32::MAX),
                    },
                )
            })
            .collect();
        let original_at = u32::try_from(composed.original_at).unwrap_or(u32::MAX);
        branches.extend(
            nested
                .iter()
                .map(|(index, span)| (*index, shift(*span, original_at))),
        );
        Ok(Rendered {
            text: composed.text,
            branches,
        })
    }

    /// The branch that keeps the original: the site's own bytes with every
    /// site nested inside it already rendered, and their branch ranges
    /// shifted to where they landed.
    fn original_branch(&self, node: &Node<Placement>) -> Result<Rendered, InstrumentError> {
        let bounds = |from: u32, to: u32| {
            Span::new(from, to)
                .map_err(|error| self.error(InstrumentErrorKind::SiteConflict, error.to_string()))
        };
        let mut text = String::new();
        let mut branches: Vec<(u32, Span)> = Vec::new();
        let mut cursor = node.span.start;
        for child in &node.children {
            text.push_str(self.slice(bounds(cursor, child.span.start)?)?);
            let rendered = self.render(child)?;
            let at = u32::try_from(text.len()).unwrap_or(u32::MAX);
            branches.extend(
                rendered
                    .branches
                    .iter()
                    .map(|(index, span)| (*index, shift(*span, at))),
            );
            text.push_str(&rendered.text);
            cursor = child.span.end;
        }
        text.push_str(self.slice(bounds(cursor, node.span.end)?)?);
        Ok(Rendered { text, branches })
    }

    /// One alternative: the pristine site with exactly this edit applied,
    /// folded onto one line.
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

    /// One `#[allow(warnings)]` insertion per function that holds a guard.
    ///
    /// An insertion that would land inside a rewrite site is dropped: the
    /// site's own text is composed here, not spliced, so an insertion into
    /// it would be applied twice. That costs the guards in such a function
    /// their lint suppression and nothing else, and the case needs a
    /// function declared inside an expression to arise at all.
    fn allow_splices(placements: &[Placement], roots: &[Node<Placement>]) -> Vec<Splice> {
        let offsets: BTreeSet<u32> = placements
            .iter()
            .filter_map(|placement| placement.hint.allow_at)
            .filter(|offset| {
                !roots
                    .iter()
                    .any(|root| root.span.start < *offset && *offset < root.span.end)
            })
            .collect();
        offsets
            .into_iter()
            .map(|offset| Splice {
                span: Span {
                    start: offset,
                    end: offset,
                },
                original: Vec::new(),
                replacement: ALLOW_ATTRIBUTE.as_bytes().to_vec(),
            })
            .collect()
    }
}
