// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rewriting a file so that every compilable mutant of it lives in the file at once, dormant behind a guard.

mod guards;
mod observable;
mod runtime;
pub mod witness;

/// The name a generated module of `stem` can take in `text`, dodging every identifier the file spells.
#[must_use]
pub fn module_named_for(text: &str, stem: &str) -> String {
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
    ACTIVE_ENV, CATALOG_ENV, COMPILED_CATALOG_ENV, MODULE_STEM, RUNTIME_MARKER, Rendering,
    STALE_CATALOG_EXIT, TOUCH_ENV, TOUCH_UNAVAILABLE_EXIT, module_name, render,
};

/// The first words the runtime prints before it exits [`runtime::STALE_CATALOG_EXIT`].
///
/// A test binary the engine starts itself is recognised by that exit code. One
/// cargo starts for it — a documentation example — reaches the engine as
/// cargo's own 101, which is the code a failing test has, so a tree that had
/// been rebuilt behind the run's back would look exactly like a kill. What
/// cargo does pass through is the output, and this is the engine's own
/// sentence in it.
pub const STALE_CATALOG_MARKER: &str = "rust-mutants: this binary was built from catalog ";

/// Every lint a guard's own text can trip, which the attribute it carries turns off.
///
/// A crate that *forbids* one of these forbids the attribute too — `forbid`
/// is the level `allow` cannot override — so no mutant of it would compile,
/// and a run says so by name rather than refusing every candidate.
pub const GUARD_NOISE_LINTS: [&str; 9] = [
    "warnings",
    "unused",
    "unused_qualifications",
    "unfulfilled_lint_expectations",
    "clippy::all",
    "clippy::pedantic",
    "clippy::restriction",
    "clippy::nursery",
    "clippy::cargo",
];

/// The text inserted before the innermost function holding a guard, so that a guard's own lint noise never trips a crate's deny policy anywhere else. It holds no line break.
pub const ALLOW_ATTRIBUTE: &str = "#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] ";

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

/// Moves a range along by `by` bytes.
const fn shift(span: Span, by: u32) -> Span {
    Span {
        start: span.start.saturating_add(by),
        end: span.end.saturating_add(by),
    }
}

/// What one file is rewritten with: the mutants, the shape they nest in, and the markers its branch proofs put in it.
#[derive(Debug, Clone, Copy)]
struct Planted<'a> {
    /// The mutants placed in the file.
    placements: &'a [Placement],
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
    ///
    /// It is what the tree does rather than what it was offered: a form that
    /// cannot compare reports nothing here, so a run reading it back never
    /// rests a proof on a comparison no guard makes.
    pub compared: Vec<u32>,
    /// Every marker this text holds the call for, ascending, which is not every marker it was given: a body inside a guard's own site takes none.
    pub marked: Vec<u32>,
    /// The name the runtime module took, empty when none was generated.
    pub module: String,
    /// Whether anything was rewritten. A file with no mutants comes back byte for byte, without a runtime: an unused module would only be noise, and a file cargo did not have to recompile is one this run does not pay for.
    pub instrumented: bool,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Rewrites one file so that every placed mutant lives in it behind a guard.
///
/// # Errors
/// See [`InstrumentErrorKind`].
pub fn instrument_file(file: &Instrumenting<'_>) -> Result<FileOutput, InstrumentError> {
    let Instrumenting {
        path,
        source,
        placements,
        markers,
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
    if placements.is_empty() {
        return Ok(FileOutput {
            path: path.to_owned(),
            text: text.to_owned(),
            guards: Vec::new(),
            branches: Vec::new(),
            compared: Vec::new(),
            marked: Vec::new(),
            module: String::new(),
            instrumented: false,
        });
    }
    let file = File {
        path,
        text,
        module: module_name(path, text),
        comparable,
        probed,
    };
    file.check_placements(placements)?;
    let forest = file.forest(placements)?;
    let markers = File::markable(markers, &forest);

    let Rewritten {
        mut text,
        branches,
        compared,
    } = file.rewrite(
        source,
        &Planted {
            placements,
            forest: &forest,
            markers: &markers,
        },
    )?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&render(&Rendering {
        module: &file.module,
        catalog_digest,
        placements,
        markers: &markers,
        newline: file.newline(),
    }));

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
        compared: compared.into_iter().collect(),
        marked: markers.iter().map(|marker| marker.index).collect(),
        module: file.module,
        instrumented: true,
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

    /// Every placement must name bytes this file really holds, and an index the runtime can tell apart from its sentinels.
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

    /// Arranges the sites by containment. Sites come from the syntax tree, so they nest; a partial overlap is an engine bug and is refused.
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

    /// Applies every guard and every allow attribute to the file's bytes, and reports where each alternative landed in the result.
    /// The markers that can be written where they are, which is every one outside every guard's own site.
    ///
    /// A guard replaces its site with `if active { alternative } else {
    /// original }`, and a marker strictly inside that site would have to be
    /// written into both halves rather than spliced once. A body inside a
    /// guard's site is left unmarked instead, which costs its claim the marker
    /// and leaves the coverage region as the premise it rests on. A marker at
    /// a site's own first byte is not inside it: the splice is an insertion,
    /// it sorts before the replacement, and what it writes lands where the
    /// body's first statement was about to be.
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
        Splice {
            span: Span {
                start: marker.at,
                end: marker.at,
            },
            original: Vec::new(),
            replacement: format!(
                "{}{}::body({}); ",
                "super::".repeat(usize::try_from(marker.super_depth).unwrap_or(0)),
                self.module,
                marker.index
            )
            .into_bytes(),
        }
    }

    fn rewrite(&self, source: &[u8], planted: &Planted<'_>) -> Result<Rewritten, InstrumentError> {
        let Planted {
            placements,
            forest,
            markers,
        } = *planted;
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
            let at = u32::try_from(text.len()).unwrap_or(u32::MAX);
            branches.extend(
                rendered
                    .branches
                    .iter()
                    .map(|(index, span)| (*index, shift(*span, at))),
            );
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

    /// One `#[allow(warnings)]` insertion per function that holds a guard.
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
