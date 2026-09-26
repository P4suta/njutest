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
    crate::parsing::apart(|parsing| runtime::module_named(parsing, text, stem))?
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::interval::{self, Item, Node};
use crate::span::Span;
use crate::splice::{Splice, apply, count_lines};
use crate::syntax::branch::Marker;
use crate::syntax::{Form, Found, SiteHint};

pub use runtime::{
    ACTIVE_ENV, CATALOG_ENV, COMPILED_CATALOG_ENV, CRASH_EXIT, CRASH_NONCE_ENV, CRASH_NOTICE_ENV,
    CRASH_NOTICE_SCHEMA, CRASHED_CALL, DELAY_ENV, FAULT_ENV, INJECTED, INJECTED_CALL, MODULE_STEM,
    ModuleNameError, ORPHAN_PREFIX, RUNTIME_MARKER, Rendering, RuntimeRenderError,
    STALE_CATALOG_EXIT, STEP_BEAT_ENV, STEP_NONCE_ENV, STEP_NOTICE_ENV, STEP_NOTICE_SCHEMA,
    STEP_PROTOCOL_EXIT, STEP_STATE_ENV, STEP_STATE_SCHEMA, STEPS_ENV, STOP_SCHEMA, TOUCH_ENV,
    TOUCH_ITEMS_ENV, TOUCH_UNAVAILABLE_EXIT, WATCHED_ENV, module_name, render,
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

/// One item of a file whose body a test can enter: a function, a method, or a constant, as the instrumenter found it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemBody {
    /// The item index its entry marker names.
    pub index: u32,
    /// The item as a reader writes it: `mod::path::Type::method`, the same path a mutant inside it names.
    pub name: String,
    /// The bytes the whole item covers in the pristine file.
    pub span: Span,
    /// The bytes its body covers in the pristine file.
    pub body: Span,
    /// Whether the instrumenter wrote an entry marker into it, which it cannot do into a body the compiler may evaluate at compile time.
    pub measurable: bool,
}

/// Every item of one file, numbered from `first_item` in the order the instrumenter plants their entry markers.
///
/// # Errors
/// [`InstrumentErrorKind::SourceMismatch`] when the source is not UTF-8 or not a Rust file, or an item index would not fit the runtime's window.
pub fn items(path: &str, source: &[u8], first_item: u32) -> Result<Vec<ItemBody>, InstrumentError> {
    let text = std::str::from_utf8(source).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source is not valid UTF-8: {error}"),
        )
    })?;
    crate::parsing::apart(|parsing| steps::plant(parsing, text, MODULE_STEM, first_item))
        .map_err(|unread| {
            InstrumentError::unread(InstrumentErrorKind::SourceMismatch, path, &unread)
        })?
        .map(|planted| planted.items)
        .map_err(|error| {
            InstrumentError::new(
                InstrumentErrorKind::SourceMismatch,
                path,
                format!("the items cannot be numbered: {error}"),
            )
        })
}

/// One file whose items are to be numbered: where it is, who compiles it, and its pristine bytes.
#[derive(Debug, Clone, Copy)]
pub struct ItemSource<'a> {
    /// The workspace-relative path.
    pub path: &'a str,
    /// The package whose unit compiled it.
    pub package: &'a str,
    /// The pristine bytes.
    pub source: &'a [u8],
}

/// Every item of a tree, numbered, and the index each file's items start from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemCatalog {
    /// Every item, by item index.
    pub items: Vec<crate::touch::Item>,
    /// The item index each file's first item takes, by workspace-relative path.
    pub first: BTreeMap<String, u32>,
}

impl ItemCatalog {
    /// The position of `item` among every cataloged item of its file, in catalog order: the one definition an entered union, a sealed placeholder, and an audit all name an item by.
    #[must_use]
    pub fn ordinal(&self, item: &crate::touch::Item) -> Option<u32> {
        let first = self.first.get(&item.path)?;
        item.index.checked_sub(*first)
    }

    /// The portable name of the item at `index`, when the catalog holds one.
    #[must_use]
    pub fn item_ref(&self, index: u32) -> Option<crate::touch::ItemRef> {
        let at = match usize::try_from(index) {
            Ok(at) => at,
            Err(_beyond_this_target) => return None,
        };
        let item = self.items.get(at)?;
        Some(crate::touch::ItemRef {
            package: item.package.clone(),
            path: item.path.clone(),
            ordinal: self.ordinal(item)?,
        })
    }
}

/// Numbers every item of `files` densely, in the order the files are given, so a marker's index names one item of the whole tree.
///
/// # Errors
/// What [`items`] refuses about any one file, or a tree whose items do not fit the runtime's window.
pub fn catalog_items(files: &[ItemSource<'_>]) -> Result<ItemCatalog, InstrumentError> {
    let mut catalog = ItemCatalog::default();
    let mut next: u32 = 0;
    for file in files {
        catalog.first.insert(file.path.to_owned(), next);
        let found = items(file.path, file.source, next)?;
        let overflow = || {
            InstrumentError::new(
                InstrumentErrorKind::IndexReserved,
                file.path,
                format!("{} more items do not fit the runtime's window", found.len()),
            )
        };
        let count = match u32::try_from(found.len()) {
            Ok(count) => count,
            Err(_too_many) => return Err(overflow()),
        };
        next = next.checked_add(count).ok_or_else(overflow)?;
        catalog
            .items
            .extend(found.into_iter().map(|body| crate::touch::Item {
                index: body.index,
                package: file.package.to_owned(),
                path: file.path.to_owned(),
                name: body.name,
                span: body.span,
                body: body.body,
                measurable: body.measurable,
            }));
    }
    Ok(catalog)
}

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
    /// Whether its guard is carried into every alternative of a site it nests in, so it can be active beside a mutation there.
    pub carried: bool,
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

/// One site an alternative is written for: where it is, its pristine text, and the guards carried into every alternative of it.
#[derive(Clone, Copy)]
struct Around<'a> {
    site: Span,
    text: &'a str,
    carried: &'a [Carried],
}

/// A guard carried into the alternatives of the site it nests in: where it is, its rendered text, and the faults it can activate.
struct Carried {
    span: Span,
    text: String,
    faults: Vec<u32>,
}

/// What precedes an edit in its alternative, and which carried guards it holds.
struct Headed<'c> {
    head: String,
    in_head: Vec<u32>,
    kept: Option<&'c Carried>,
}

/// One alternative's text, and the faults whose guards it carries.
struct Written {
    text: String,
    carries: Vec<u32>,
}

/// A rewritten file: its text and where every alternative landed in it.
struct Rewritten {
    text: String,
    branches: Vec<Branch>,
    compared: BTreeSet<u32>,
    beside: BTreeSet<(u32, u32)>,
}

/// A rendered site: its text, where each alternative sits in it, and which of them the guard evaluates beside what it replaces.
struct Rendered {
    text: String,
    branches: Vec<(u32, Span)>,
    compared: BTreeSet<u32>,
    beside: BTreeSet<(u32, u32)>,
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
    /// Every mutation whose branch in this text carries a fault's guard, with that fault, ascending: the only pairs a fault can be active beside.
    pub beside: Vec<(u32, u32)>,
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
    /// The rewritten file does not read as Rust, down to what every identity macro holds: a guard changed how the syntax around it reads.
    Unparsable,
    /// Reading the file would take its reading thread's locations past what they address.
    ReadingExhausted,
    /// The thread the file is read on could not be started or did not finish.
    ReadingThread,
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
            Self::Unparsable => error::INSTRUMENT_UNPARSABLE,
            Self::ReadingExhausted => error::READING_EXHAUSTED,
            Self::ReadingThread => error::READING_THREAD,
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
    /// Why text read while instrumenting `path` failed: a syntax error as `syntax`, and a reading that could not happen at all as what stopped it.
    fn unread(
        syntax: InstrumentErrorKind,
        path: &str,
        unread: &crate::parsing::ReadingError,
    ) -> Self {
        let kind = match unread {
            crate::parsing::ReadingError::Syntax { .. } => syntax,
            crate::parsing::ReadingError::Exhausted { .. } => InstrumentErrorKind::ReadingExhausted,
            crate::parsing::ReadingError::ThreadUnavailable { .. }
            | crate::parsing::ReadingError::ThreadPanicked
            | crate::parsing::ReadingError::Unbudgeted => InstrumentErrorKind::ReadingThread,
        };
        Self::new(kind, path, unread.to_string())
    }

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
            carried: one.candidate.rule.family.carried_beside(),
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
    /// The item index the first item of this file takes, which is where its entry markers start counting.
    pub first_item: u32,
    /// The absolute directory a process of the tree that lost the run's environment says so in.
    pub watched: &'a str,
}

/// The pristine source after process-wide checkpoints have been inserted and every catalog position has been mapped into that intermediate source.
struct Checkpointed {
    source: Vec<u8>,
    module: String,
    placements: Vec<Placement>,
    markers: Vec<Marker>,
    items: u32,
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
    parsing: &crate::parsing::Parsing,
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
        first_item,
        watched: _watched,
    } = *file;
    let planted = steps::plant(parsing, text, &module, first_item).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the step checkpoints cannot be placed: {error}"),
        )
    })?;
    let items = u32::try_from(planted.items.len()).map_err(|_overflow| {
        InstrumentError::new(
            InstrumentErrorKind::IndexReserved,
            path,
            format!(
                "{} items do not fit the runtime's window",
                planted.items.len()
            ),
        )
    })?;
    let (source, offsets) = apply(source, &planted.splices).map_err(|error| {
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
        items,
    })
}

/// Rewrites one file so that every placed mutant lives in it behind a guard.
///
/// `bytes` as text, or the refusal of kind `kind` saying `what` is not.
fn text_of<'a>(
    bytes: &'a [u8],
    (kind, path): (InstrumentErrorKind, &str),
    what: &str,
) -> Result<&'a str, InstrumentError> {
    std::str::from_utf8(bytes).map_err(|error| {
        InstrumentError::new(kind, path, format!("{what} is not valid UTF-8: {error}"))
    })
}

/// # Errors
/// See [`InstrumentErrorKind`].
pub fn instrument_file(file: &Instrumenting<'_>) -> Result<FileOutput, InstrumentError> {
    crate::parsing::apart(|parsing| instrument_with(parsing, file)).map_err(|unread| {
        InstrumentError::unread(InstrumentErrorKind::SourceMismatch, file.path, &unread)
    })?
}

/// [`instrument_file`], reading with `parsing` on the thread already reading.
fn instrument_with(
    parsing: &crate::parsing::Parsing,
    file: &Instrumenting<'_>,
) -> Result<FileOutput, InstrumentError> {
    let Instrumenting {
        path,
        source,
        placements,
        markers: _original_markers,
        comparable,
        probed,
        catalog_digest,
        first_item,
        watched,
    } = *file;
    let text = text_of(
        source,
        (InstrumentErrorKind::SourceMismatch, path),
        "the source",
    )?;
    let module = named(parsing, path, text)?;
    check_pristine(parsing, file, text, &module)?;
    let checkpointed = checkpointed(parsing, file, text, module)?;
    let bounded_text = text_of(
        &checkpointed.source,
        (InstrumentErrorKind::SpliceFailed, path),
        "the checkpointed source",
    )?;
    let worker = File {
        parsing,
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
        beside,
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
    worker.reparsed(&text)?;
    worker.append_runtime(
        &mut text,
        &Rendering {
            module: &worker.module,
            catalog_digest,
            placements: &checkpointed.placements,
            markers: &markers,
            first_item,
            item_count: checkpointed.items,
            newline: worker.newline(),
            watched,
        },
    )?;
    Ok(FileOutput {
        path: path.to_owned(),
        text,
        guards: guards_of(placements),
        branches,
        compared: compared.into_iter().collect(),
        marked: markers.iter().map(|marker| marker.index).collect(),
        beside: beside.into_iter().collect(),
        module: worker.module,
        instrumented: true,
    })
}

/// The runtime module name `text` can take, or why its tokens could not be read.
fn named(
    parsing: &crate::parsing::Parsing,
    path: &str,
    text: &str,
) -> Result<String, InstrumentError> {
    runtime::module_name_in(parsing, path, text).map_err(|error| {
        InstrumentError::new(
            InstrumentErrorKind::SourceMismatch,
            path,
            format!("the source token stream is invalid: {error}"),
        )
    })
}

fn check_pristine(
    parsing: &crate::parsing::Parsing,
    file: &Instrumenting<'_>,
    text: &str,
    module: &str,
) -> Result<(), InstrumentError> {
    File {
        parsing,
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
        carried: placement.carried,
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
    parsing: &'a crate::parsing::Parsing,
    path: &'a str,
    text: &'a str,
    module: String,
    /// Every mutant of this file whose guard may compare its two branches.
    comparable: &'a BTreeSet<u32>,
    /// Every return replacement of this file whose guard may ask what the value it replaces already held.
    probed: &'a BTreeMap<u32, crate::probe::Question>,
}

/// Whether `text` reads as Rust down to what every identity macro of the runtime module `module` holds, which the compiler reads only once it expands them.
///
/// One parse reads all of it: every call of the macro is read as the parentheses it expands to.
///
/// # Errors
/// The first place that does not read, as the parser says it, at the line and column it has in `text`.
pub(crate) fn read_through(
    parsing: &crate::parsing::Parsing,
    text: &str,
    module: &str,
) -> Result<(), crate::parsing::ReadingError> {
    read_through_with(text, module, |unwrapped| {
        parsing.file(unwrapped).map(|_file| ())
    })
}

/// [`read_through`] with the parser given, which it calls exactly once whatever the guards hold.
fn read_through_with(
    text: &str,
    module: &str,
    mut parse: impl FnMut(&str) -> Result<(), crate::parsing::ReadingError>,
) -> Result<(), crate::parsing::ReadingError> {
    parse(&unwrapped(text, module))
}

/// `text` with the path and `!` of every call of `module`'s identity macro written as spaces, so the call reads as the parentheses it expands to and every byte keeps its place.
fn unwrapped(text: &str, module: &str) -> String {
    const OUTER: &str = "super::";
    let call = format!("{module}::value!");
    let mut kept = String::with_capacity(text.len());
    let mut from = 0_usize;
    while let Some((before, rest)) = text.get(from..).and_then(|rest| rest.split_once(&call)) {
        let mut path_start = before.len();
        while before
            .get(..path_start)
            .is_some_and(|head| head.ends_with(OUTER))
        {
            path_start = path_start.saturating_sub(OUTER.len());
        }
        kept.push_str(before.get(..path_start).unwrap_or_default());
        let blanked = before
            .len()
            .saturating_sub(path_start)
            .saturating_add(call.len());
        kept.extend(std::iter::repeat_n(' ', blanked));
        from = text.len().saturating_sub(rest.len());
    }
    kept.push_str(text.get(from..).unwrap_or_default());
    kept
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

    /// Whether the rewritten file reads as Rust down to what every identity macro holds, which the compiler reads only once it expands them.
    fn reparsed(&self, text: &str) -> Result<(), InstrumentError> {
        read_through(self.parsing, text, &self.module).map_err(|error| self.unparsable(&error))
    }

    fn unparsable(&self, error: &crate::parsing::ReadingError) -> InstrumentError {
        match error {
            crate::parsing::ReadingError::Syntax {
                line,
                column,
                message,
            } => self.error(
                InstrumentErrorKind::Unparsable,
                format!(
                    "the rewritten file does not read as Rust at line {line}, column {column}: {message}"
                ),
            ),
            crate::parsing::ReadingError::Exhausted { .. }
            | crate::parsing::ReadingError::ThreadUnavailable { .. }
            | crate::parsing::ReadingError::ThreadPanicked
            | crate::parsing::ReadingError::Unbudgeted => {
                InstrumentError::unread(InstrumentErrorKind::Unparsable, self.path, error)
            }
        }
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
        let mut beside = BTreeSet::new();
        for root in forest.roots() {
            let rendered = self.render(root)?;
            splices.push(self.splice(root.span, rendered.text.clone())?);
            compared.extend(rendered.compared.iter().copied());
            beside.extend(rendered.beside.iter().copied());
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
            beside,
        })
    }

    /// Renders one site: its alternatives, then its original branch with the sites nested inside it already rendered.
    fn render(&self, node: &Node<Placement>) -> Result<Rendered, InstrumentError> {
        let Rendered {
            text: original,
            branches: nested,
            mut compared,
            mut beside,
        } = self.original_branch(node)?;

        let site = self.slice(node.span)?;
        let carried = self.carried(node)?;
        let mut alternatives = Vec::with_capacity(node.alternatives.len());
        for placement in &node.alternatives {
            let written = self.alternative(
                &Around {
                    site: node.span,
                    text: site,
                    carried: &carried,
                },
                placement,
            )?;
            beside.extend(
                written
                    .carries
                    .iter()
                    .map(|fault| (placement.index, *fault)),
            );
            alternatives.push(guards::Alternative {
                index: placement.index,
                text: written.text,
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
            beside,
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
        let mut beside = BTreeSet::new();
        let mut cursor = node.span.start;
        for child in &node.children {
            text.push_str(self.slice(bounds(cursor, child.span.start)?)?);
            let rendered = self.render(child)?;
            let at = self.offset(text.len(), "nested guard start")?;
            for (index, span) in rendered.branches {
                branches.push((index, self.shifted(span, at, "nested branch")?));
            }
            compared.extend(rendered.compared);
            beside.extend(rendered.beside);
            text.push_str(&rendered.text);
            cursor = child.span.end;
        }
        text.push_str(self.slice(bounds(cursor, node.span.end)?)?);
        Ok(Rendered {
            text,
            branches,
            compared,
            beside,
        })
    }

    /// One alternative: the pristine site with exactly this edit applied, folded onto one line, with every carried guard the edit keeps the bytes of rendered where those bytes are.
    fn alternative(
        &self,
        around: &Around<'_>,
        placement: &Placement,
    ) -> Result<Written, InstrumentError> {
        let Around {
            site,
            text: site_text,
            carried,
        } = *around;
        let Headed {
            head,
            in_head,
            kept,
        } = self.headed(site, carried, placement)?;
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
        let in_replacement: Vec<u32> = kept.map(|one| one.faults.clone()).unwrap_or_default();
        let replacement = match kept {
            Some(one) => {
                let (_, rest) = replacement.split_at(self.slice(one.span)?.len());
                format!("{}{rest}", one.text)
            }
            None => replacement.to_owned(),
        };
        let resolved = replacement
            .replace(
                INJECTED_CALL,
                &guards::named(&self.module, placement.hint.super_depth, "injected"),
            )
            .replace(
                CRASHED_CALL,
                &guards::named(&self.module, placement.hint.super_depth, "crashed_after"),
            );
        let replacement = resolved.as_str();
        if placement.hint.form == Form::M {
            return Ok(Written {
                text: replacement.to_owned(),
                carries: in_replacement,
            });
        }
        let text = format!("{head}{replacement}{tail}");
        debug_assert!(!site_text.is_empty() || text.is_empty());
        if text.trim().is_empty() {
            return Ok(Written {
                text: String::new(),
                carries: Vec::new(),
            });
        }
        let text = crate::flatten::flatten_with(self.parsing, &text).map_err(|error| {
            self.error(
                InstrumentErrorKind::FlattenFailed,
                format!(
                    "the {} alternative of mutant {} cannot be folded onto one line: {error}",
                    placement.hint.form, placement.index
                ),
            )
        })?;
        Ok(Written {
            text,
            carries: in_head.into_iter().chain(in_replacement).collect(),
        })
    }

    /// Every child of `node` whose every alternative is a fault, rendered, which is what every alternative of `node` that keeps its bytes carries.
    fn carried(&self, node: &Node<Placement>) -> Result<Vec<Carried>, InstrumentError> {
        let mut carried = Vec::new();
        for child in node
            .children
            .iter()
            .filter(|child| child.alternatives.iter().all(|placement| placement.carried))
        {
            carried.push(Carried {
                span: child.span,
                text: self.render(child)?.text,
                faults: child
                    .alternatives
                    .iter()
                    .map(|placement| placement.index)
                    .collect(),
            });
        }
        Ok(carried)
    }

    /// The pristine bytes of a site before the edit, with every carried guard wholly inside them rendered in place, and the carried guard the edit's replacement begins with.
    fn headed<'c>(
        &self,
        site: Span,
        carried: &'c [Carried],
        placement: &Placement,
    ) -> Result<Headed<'c>, InstrumentError> {
        let mut head = String::new();
        let mut cursor = site.start;
        let mut kept: Option<&Carried> = None;
        let mut in_head: Vec<u32> = Vec::new();
        for one in carried {
            if one.span.end <= placement.edit.start {
                head.push_str(self.slice(Span::new(cursor, one.span.start).map_err(
                    |error| self.error(InstrumentErrorKind::SiteConflict, error.to_string()),
                )?)?);
                head.push_str(&one.text);
                in_head.extend(one.faults.iter().copied());
                cursor = one.span.end;
            } else if one.span.start == placement.edit.start
                && placement
                    .replacement
                    .starts_with(self.slice(one.span)?.as_bytes())
            {
                kept = Some(one);
            }
        }
        head.push_str(
            self.slice(Span::new(cursor, placement.edit.start).map_err(|error| {
                self.error(InstrumentErrorKind::SiteConflict, error.to_string())
            })?)?,
        );
        Ok(Headed {
            head,
            in_head,
            kept,
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

#[cfg(test)]
mod tests {
    use super::{read_through_with, unwrapped};

    /// A function whose tail holds `depth` guards, each inside the original branch of the one around it, the innermost holding `inner`.
    fn nested(depth: usize, inner: &str) -> String {
        let mut guard = inner.to_owned();
        for index in 0..depth {
            guard = format!(
                "super::rt::value!(if super::rt::active({index}) {{ 0 }} else {{ {guard} }})"
            );
        }
        format!("mod m {{\n    fn f() -> u8 {{\n        {guard}\n    }}\n}}\n")
    }

    /// `text` read as a file on a reading thread, for the laws that count or place the reads.
    fn file_of(text: &str) -> Result<(), crate::parsing::ReadingError> {
        crate::parsing::apart(|parsing| parsing.file(text).map(|_file| ()))?
    }

    #[test]
    fn one_parse_reads_every_identity_macro_however_deep_the_guards_nest() {
        for depth in [0, 1, 8, 64] {
            let mut parses = 0_usize;
            let read = read_through_with(&nested(depth, "1"), "rt", |text| {
                parses = parses.saturating_add(1);
                file_of(text)
            });
            assert!(read.is_ok(), "{depth} nested guards read: {read:?}");
            assert_eq!(
                parses, 1,
                "reading what {depth} nested identity macros hold is one parse of the file, not one \
                 more for every guard a byte sits inside"
            );
        }
    }

    #[test]
    fn a_guard_that_breaks_deep_inside_is_found_where_it_is() {
        let text = nested(16, "{ 1 } + ");
        let read = read_through_with(&text, "rt", file_of);
        assert!(
            matches!(
                read,
                Err(crate::parsing::ReadingError::Syntax { line: 3, .. })
            ),
            "the innermost guard does not read, and the error names its line in the file: {read:?}"
        );
    }

    #[test]
    fn a_call_is_blanked_to_its_parentheses_and_every_byte_keeps_its_place() {
        let text = "a(super::super::rt::value!(b), rt::value!(c), other::value!(d))";
        let read = unwrapped(text, "rt");
        assert_eq!(read.len(), text.len(), "{read}");
        assert_eq!(
            read,
            format!(
                "a({}(b), {}(c), other::value!(d))",
                " ".repeat("super::super::rt::value!".len()),
                " ".repeat("rt::value!".len())
            ),
            "only the module's own identity macro is read as its parentheses, `super::` and all"
        );
    }
}
