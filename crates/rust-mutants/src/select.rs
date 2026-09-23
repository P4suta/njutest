// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which targets a change can be noticed by, decided from the items each target entered on a measured tree.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

use crate::span::Span;
use crate::touch::{Item, Steadiness, Touched};

/// One file as it was measured and as it is now, holding measured bytes proven to be the ones the measurement read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revision<'a> {
    path: &'a str,
    old: &'a str,
    new: &'a str,
}

/// One file whose bytes differ from the measured tree's, as far as the measurement can place the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed<'a> {
    /// The file is there in both, and its measured bytes are the ones the measurement digested.
    Revised(Revision<'a>),
    /// The file was added or deleted, so no measured item of it names what changed.
    Whole {
        /// The workspace-relative path with forward slashes.
        path: &'a str,
    },
    /// The bytes offered as the measured file are not the ones the measurement digested, so nothing measured is about them.
    Unproven {
        /// The workspace-relative path with forward slashes.
        path: &'a str,
    },
}

impl<'a> Changed<'a> {
    /// The change to `path`, given the digest the measurement recorded for it and its bytes then and now, where there are any.
    #[must_use]
    pub fn read(path: &'a str, measured: &str, old: Option<&'a str>, new: Option<&'a str>) -> Self {
        match (old, new) {
            (Some(old), _) if crate::id::digest(old.as_bytes()) != measured => {
                Self::Unproven { path }
            }
            (Some(old), Some(new)) => Self::Revised(Revision { path, old, new }),
            (None, _) | (Some(_), None) => Self::Whole { path },
        }
    }

    /// The file the change is to.
    #[must_use]
    pub const fn path(&self) -> &'a str {
        match self {
            Self::Revised(Revision { path, .. })
            | Self::Whole { path }
            | Self::Unproven { path } => path,
        }
    }
}

/// Why every target runs, whatever it entered: a change the measurement cannot place in the body of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Everything {
    /// A manifest, lock file, build script, toolchain file, or cargo or runner configuration changed, which can change what every target compiles to.
    Build {
        /// The file.
        path: String,
    },
    /// A file changed that holds no measured item, so nothing says who ran it.
    Unitemized {
        /// The file.
        path: String,
    },
    /// A file was added or deleted.
    Whole {
        /// The file.
        path: String,
    },
    /// The bytes offered as the measured file are not the ones the measurement read.
    Unproven {
        /// The file.
        path: String,
    },
    /// One version of the file does not parse, so its items cannot be compared.
    Unparsed {
        /// The file.
        path: String,
    },
    /// Something outside every measured body changed: an item, a signature, an attribute, a `use`, a type, a comment's effect on a doc, or a body the guards cannot record.
    Skeleton {
        /// The file.
        path: String,
        /// The 1-based line of the measured file at which the two first differ.
        line: usize,
    },
    /// A changed body holds what can reach past it: an `impl`, an exported symbol, or a macro that may expand to either.
    Escapes {
        /// The item, as a reader writes it.
        item: String,
        /// The token that can reach past the body.
        by: String,
    },
    /// A body the guards cannot record moved or changed, and a `const fn` called at run time says where it is when it panics.
    Unmeasurable {
        /// The item, as a reader writes it.
        item: String,
    },
    /// Something a macro outside the standard set reads moved, and what it expands to may say where it was: a derive's `unwrap`, an attribute that records its line.
    Located {
        /// The file.
        path: String,
        /// The 1-based line of the measured file where it was.
        line: usize,
    },
}

/// What a selection reads of one measurement.
#[derive(Debug, Clone, Copy)]
pub struct Measured<'a> {
    /// What each target entered, and the items of the measured tree.
    pub touched: &'a Touched,
    /// Whether each target's reach held on a second run.
    pub standing: &'a BTreeMap<String, Steadiness>,
    /// What each package declares over the standard names; a package missing here hides every one.
    pub shadows: &'a BTreeMap<String, Shadows>,
}

/// What a selection decided for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// The target's tests entered none of the changed items on the measured tree, and its measurement held on a second run, so running it would ask nothing the change could answer.
    Skip,
    /// The target runs, and why.
    Run(Why),
}

/// Why one target runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Its tests entered these changed items.
    Entered(BTreeSet<u32>),
    /// The change is one no measurement can place.
    Everything(Everything),
    /// Its reach was not shown to be a function of the target: a second run moved it, or nothing compared a second run with the first.
    Unestablished(Steadiness),
    /// The measurement holds nothing about the target.
    Unmeasured,
}

/// What one selection decided, total over the targets that exist now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    decided: BTreeMap<String, Decided>,
}

impl Selection {
    /// What was decided for every target that exists now.
    #[must_use]
    pub const fn decided(&self) -> &BTreeMap<String, Decided> {
        &self.decided
    }

    /// The targets proved unable to notice the change, which is all a caller may leave out.
    pub fn skippable(&self) -> impl Iterator<Item = &str> {
        self.decided
            .iter()
            .filter(|(_, decided)| **decided == Decided::Skip)
            .map(|(target, _)| target.as_str())
    }
}

/// The standard names one package declares something else under, so that where they appear in it they may not be the standard ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadows {
    /// Every standard macro, attribute, or derive name the package declares or imports from outside the standard library.
    names: BTreeSet<String>,
    /// Whether the package imports what it cannot name: a glob from outside itself, or every macro of a `#[macro_use] extern crate`.
    all: bool,
}

/// The standard names a package can declare something else under and change what a body, an attribute, or a derive expands to.
const SHADOWABLE: [&str; 13] = [
    "test",
    "bench",
    "derive",
    "global_allocator",
    "Debug",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "Default",
];

/// The crates a path is the standard library's by starting with.
const STANDARD_CRATES: [&str; 3] = ["std", "core", "alloc"];

impl Shadows {
    /// What the sources of one package, every file of it, declare over the standard names; a source that does not lex hides every name.
    #[must_use]
    pub fn of<'a>(sources: impl IntoIterator<Item = &'a str>) -> Self {
        let mut shadows = Self::default();
        for source in sources {
            let Some(leaves) = lexed(source) else {
                shadows.all = true;
                continue;
            };
            shadows.read(&leaves);
        }
        shadows
    }

    /// Whether `name`, where it appears in this package, may be something other than the standard one.
    #[must_use]
    pub fn hides(&self, name: &str) -> bool {
        self.all || self.names.contains(name)
    }

    fn watched(name: &str) -> bool {
        CONTAINED_MACROS.contains(&name) || SHADOWABLE.contains(&name)
    }

    fn read(&mut self, leaves: &[Leaf]) {
        let text = |at: usize| leaves.get(at).map(|leaf| leaf.text.as_str());
        for (at, leaf) in leaves.iter().enumerate() {
            match leaf.text.as_str() {
                "macro_rules" if text(at.saturating_add(1)) == Some("!") => {
                    if let Some(name) = text(at.saturating_add(2)) {
                        self.names.insert(name.to_owned());
                    }
                }
                "macro_use" if text(at.saturating_sub(1)) == Some("[") => {
                    let rest = leaves.get(at.saturating_add(1)..).unwrap_or_default();
                    if rest.iter().take(3).any(|one| one.text == "extern") {
                        self.all = true;
                    }
                }
                "use" => self.imported(leaves.get(at.saturating_add(1)..).unwrap_or_default()),
                _ => {}
            }
        }
    }

    fn imported(&mut self, statement: &[Leaf]) {
        let statement: Vec<&str> = statement
            .iter()
            .map(|leaf| unjoined(&leaf.text))
            .take_while(|text| *text != ";")
            .collect();
        let first = statement
            .iter()
            .find(|text| text.chars().next().is_some_and(char::is_alphabetic));
        if first.is_some_and(|first| STANDARD_CRATES.contains(first)) {
            return;
        }
        if statement.contains(&"*")
            && !first.is_some_and(|first| matches!(*first, "crate" | "self" | "super"))
        {
            self.all = true;
        }
        for name in statement {
            if Self::watched(name) {
                self.names.insert(name.to_owned());
            }
        }
    }
}

/// A token as written, without the mark that says a punctuation character is joined to the next.
fn unjoined(text: &str) -> &str {
    match text.strip_suffix('+') {
        Some(punct) if !punct.is_empty() => punct,
        Some(_) | None => text,
    }
}

/// Every token of `text`, or nothing when it does not lex.
fn lexed(text: &str) -> Option<Vec<Leaf>> {
    let Ok((base, parsed)) = crate::syntax::strip_prefix(text) else {
        return None;
    };
    let Ok(stream) = <TokenStream as std::str::FromStr>::from_str(parsed) else {
        return None;
    };
    let mut leaves = Vec::new();
    flattened(stream, base, &mut leaves)?;
    Some(leaves)
}

/// Whether `path` is a file whose change can change what every target compiles to or how it is run, whatever items it holds.
#[must_use]
pub fn builds_everything(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.toml"
            | "Cargo.lock"
            | "build.rs"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | "njutest.toml"
            | ".rust-mutants.toml"
    ) || path.starts_with(".cargo/")
        || path.contains("/.cargo/")
}

/// The std macros whose expansion is an expression of their arguments and nothing that reaches past the body they are in.
const CONTAINED_MACROS: [&str; 26] = [
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "panic",
    "unreachable",
    "todo",
    "unimplemented",
    "format",
    "format_args",
    "write",
    "writeln",
    "print",
    "println",
    "eprint",
    "eprintln",
    "dbg",
    "vec",
    "matches",
    "concat",
    "stringify",
    "line",
    "column",
    "file",
];

/// The attributes that make an item reachable by a symbol rather than by a path from the body it is written in.
const EXPORTING_ATTRIBUTES: [&str; 6] = [
    "no_mangle",
    "export_name",
    "link_section",
    "used",
    "macro_export",
    "unsafe",
];

/// One token of a file where it stands, a group's delimiters each counted as one.
#[derive(Debug, Clone)]
struct Leaf {
    text: String,
    offset: u32,
    line: usize,
    column: usize,
}

impl Leaf {
    /// The token and where a panic, a `line!()`, or a caller location inside it would say it is.
    const fn located(&self) -> (&str, usize, usize) {
        (self.text.as_str(), self.line, self.column)
    }
}

/// Whether two runs of tokens are the same tokens at the same lines and columns.
fn same_place(was: &[Leaf], is: &[Leaf]) -> bool {
    was.len() == is.len()
        && was
            .iter()
            .zip(is)
            .all(|(was, is)| was.located() == is.located())
}

fn flattened(stream: TokenStream, base: u32, into: &mut Vec<Leaf>) -> Option<()> {
    let leaf = |text: String, span: proc_macro2::Span| -> Option<Leaf> {
        Some(Leaf {
            text,
            offset: match u32::try_from(span.byte_range().start) {
                Ok(start) => base.checked_add(start)?,
                Err(_beyond_u32) => return None,
            },
            line: span.start().line,
            column: span.start().column,
        })
    };
    for tree in stream {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                into.push(leaf(open.to_owned(), group.span_open())?);
                flattened(group.stream(), base, into)?;
                into.push(leaf(close.to_owned(), group.span_close())?);
            }
            TokenTree::Ident(ident) => into.push(leaf(ident.to_string(), ident.span())?),
            TokenTree::Punct(punct) => {
                let joint = if punct.spacing() == Spacing::Joint {
                    "+"
                } else {
                    ""
                };
                into.push(leaf(format!("{}{joint}", punct.as_char()), punct.span())?);
            }
            TokenTree::Literal(literal) => into.push(leaf(literal.to_string(), literal.span())?),
        }
    }
    Some(())
}

/// A file read as its skeleton, the interiors of its outermost measurable bodies in source order, and the tokens of the bodies the guards cannot record.
struct Read {
    skeleton: Vec<Leaf>,
    interiors: Vec<Vec<Leaf>>,
    unmeasured: Vec<Vec<Leaf>>,
}

/// The bytes strictly between the braces of a body: an edit touching a brace is one to the signature.
fn interior(body: Span) -> Option<(u32, u32)> {
    let start = body.start.checked_add(1)?;
    let end = body.end.checked_sub(1)?;
    (start <= end).then_some((start, end))
}

fn body_marker(offset: u32) -> Leaf {
    Leaf {
        text: "\u{0}body".to_owned(),
        offset,
        line: 0,
        column: 0,
    }
}

fn read(
    path: &str,
    text: &str,
    bodies: &[Span],
    unmeasurable: &[Span],
) -> Result<Read, Everything> {
    let unparsed = || Everything::Unparsed {
        path: path.to_owned(),
    };
    let leaves = lexed(text).ok_or_else(unparsed)?;
    let ranges: Vec<(u32, u32)> = bodies
        .iter()
        .map(|body| interior(*body))
        .collect::<Option<_>>()
        .ok_or_else(unparsed)?;
    let mut skeleton = Vec::new();
    let mut interiors: Vec<Vec<Leaf>> = vec![Vec::new(); ranges.len()];
    let mut opened = ranges.iter().peekable();
    let mut last: Option<(usize, u32, u32)> = None;
    for leaf in leaves {
        while let Some((start, end)) = opened.next_if(|(start, _)| *start <= leaf.offset) {
            skeleton.push(body_marker(*start));
            let index = last.map_or(0, |(index, _, _)| index.saturating_add(1));
            last = Some((index, *start, *end));
        }
        match last.filter(|(_, start, end)| (*start..*end).contains(&leaf.offset)) {
            Some((index, _, _)) => interiors.get_mut(index).ok_or_else(unparsed)?.push(leaf),
            None => skeleton.push(leaf),
        }
    }
    for (start, _) in opened {
        skeleton.push(body_marker(*start));
    }
    let unmeasured = unmeasurable
        .iter()
        .map(|body| {
            skeleton
                .iter()
                .filter(|leaf| (body.start..body.end).contains(&leaf.offset))
                .cloned()
                .collect()
        })
        .collect();
    Ok(Read {
        skeleton,
        interiors,
        unmeasured,
    })
}

/// The positions in `bodies` of the outermost measurable ones, in source order: a body inside another measurable body is part of that body's interior.
fn outermost(bodies: impl IntoIterator<Item = (Span, bool)>) -> Vec<usize> {
    let mut measurable: Vec<(usize, Span)> = bodies
        .into_iter()
        .enumerate()
        .filter(|(_, (_, measurable))| *measurable)
        .map(|(at, (body, _))| (at, body))
        .collect();
    measurable.sort_by_key(|(_, body)| (body.start, std::cmp::Reverse(body.end)));
    let mut kept: Vec<(usize, Span)> = Vec::new();
    for one in measurable {
        if kept.last().is_none_or(|(_, last)| !last.contains(one.1)) {
            kept.push(one);
        }
    }
    kept.into_iter().map(|(at, _)| at).collect()
}

/// What in a changed interior can reach past the body it is written in, if anything.
fn escaping(leaves: &[Leaf], shadows: &Shadows) -> Option<String> {
    leaves.iter().enumerate().find_map(|(at, leaf)| {
        let next = leaves
            .get(at.saturating_add(1))
            .map(|one| one.text.as_str());
        let previous = at
            .checked_sub(1)
            .and_then(|before| leaves.get(before))
            .map(|one| one.text.as_str());
        if leaf.text == "impl" {
            return Some(leaf.text.clone());
        }
        if leaf.text == "use" {
            let mut local = Shadows::default();
            local.imported(leaves.get(at.saturating_add(1)..).unwrap_or_default());
            if local != Shadows::default() {
                return Some("use".to_owned());
            }
        }
        let contained =
            CONTAINED_MACROS.contains(&leaf.text.as_str()) && !shadows.hides(&leaf.text);
        if next == Some("!") && !contained {
            let first = leaf.text.chars().next();
            if first.is_some_and(|first| first.is_alphabetic() || first == '_') {
                return Some(format!("{}!", leaf.text));
            }
        }
        if previous == Some("[") && EXPORTING_ATTRIBUTES.contains(&leaf.text.as_str()) {
            return Some(format!("#[{}]", leaf.text));
        }
        None
    })
}

/// The attributes that expand to nothing a test can observe, or that the compiler itself reads.
const INERT_ATTRIBUTES: [&str; 20] = [
    "doc",
    "allow",
    "expect",
    "warn",
    "deny",
    "forbid",
    "cfg",
    "must_use",
    "inline",
    "cold",
    "test",
    "ignore",
    "should_panic",
    "non_exhaustive",
    "repr",
    "track_caller",
    "deprecated",
    "rustfmt",
    "clippy",
    "diagnostic",
];

/// The names a `derive` of the standard traits spells, whose expansions carry no location.
const STANDARD_DERIVES: [&str; 17] = [
    "Debug",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "Default",
    "std",
    "core",
    "fmt",
    "hash",
    "cmp",
    "clone",
    "marker",
    "default",
];

fn opens(text: &str) -> bool {
    matches!(text, "(" | "[" | "{")
}

fn closes(text: &str) -> bool {
    matches!(text, ")" | "]" | "}")
}

/// The index of the delimiter closing the one at `open`.
fn closing(leaves: &[Leaf], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, leaf) in leaves.iter().enumerate().skip(open) {
        if opens(&leaf.text) {
            depth = depth.checked_add(1)?;
        } else if closes(&leaf.text) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

/// Who reads what an attribute is attached to, beyond the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    /// Nothing a test can observe.
    Nobody,
    /// A macro outside the standard set, whose expansion may carry the span of what it read.
    Foreign,
    /// rustdoc, for the doc target alone.
    Rustdoc,
}

/// The attribute starting at `at`, if one does: whether it is inner, where its bracket closes, and who reads what it is attached to.
fn attribute(leaves: &[Leaf], at: usize, shadows: &Shadows) -> Option<(bool, usize, Reader)> {
    let text = |index: usize| leaves.get(index).map(|leaf| leaf.text.as_str());
    if !matches!(text(at), Some("#" | "#+")) {
        return None;
    }
    let inner = matches!(text(at.saturating_add(1)), Some("!" | "!+"));
    let open = at.saturating_add(if inner { 2 } else { 1 });
    if text(open) != Some("[") {
        return None;
    }
    let close = closing(leaves, open)?;
    let name = text(open.saturating_add(1));
    let inert = match name {
        Some("derive") => leaves
            .get(open.saturating_add(2)..close)?
            .iter()
            .filter(|leaf| {
                leaf.text
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_alphabetic() || first == '_')
            })
            .all(|leaf| {
                STANDARD_DERIVES.contains(&leaf.text.as_str()) && !shadows.hides(&leaf.text)
            }),
        Some(name) => INERT_ATTRIBUTES.contains(&name) && !shadows.hides(name),
        None => false,
    };
    let reader = match (name, inert) {
        (Some("doc"), _) => Reader::Rustdoc,
        (_, true) => Reader::Nobody,
        (_, false) => Reader::Foreign,
    };
    Some((inner, close, reader))
}

/// The index of the last token of the item or field an outer attribute ending before `from` applies to.
fn item_end(leaves: &[Leaf], from: usize, shadows: &Shadows) -> usize {
    let mut depth = 0_usize;
    let mut at = from;
    while let Some(leaf) = leaves.get(at) {
        if let Some((_, close, _)) = attribute(leaves, at, shadows) {
            at = close.saturating_add(1);
            continue;
        }
        if opens(&leaf.text) {
            depth = depth.saturating_add(1);
        } else if closes(&leaf.text) {
            let Some(inside) = depth.checked_sub(1) else {
                return at.saturating_sub(1);
            };
            depth = inside;
            if depth == 0 && leaf.text == "}" {
                return at;
            }
        } else if depth == 0 && matches!(leaf.text.as_str(), ";" | ",") {
            return at;
        }
        at = at.saturating_add(1);
    }
    leaves.len().saturating_sub(1)
}

/// The skeleton without the doc attributes nothing but rustdoc reads, each token paired with whether a macro reading the item it is part of could embed where it is.
fn visible<'a>(skeleton: &'a [Leaf], shadows: &Shadows) -> Vec<(&'a Leaf, bool)> {
    let mut located = vec![false; skeleton.len()];
    let mut doc = vec![false; skeleton.len()];
    let mut at = 0;
    while at < skeleton.len() {
        let Some((inner, close, reader)) = attribute(skeleton, at, shadows) else {
            at = at.saturating_add(1);
            continue;
        };
        match (reader, inner) {
            (Reader::Nobody, _) => {}
            (Reader::Foreign, true) => located.iter_mut().for_each(|one| *one = true),
            (Reader::Foreign, false) => {
                let end = item_end(skeleton, close.saturating_add(1), shadows);
                for one in located.iter_mut().take(end.saturating_add(1)).skip(at) {
                    *one = true;
                }
            }
            (Reader::Rustdoc, _) => {
                for one in doc.iter_mut().take(close.saturating_add(1)).skip(at) {
                    *one = true;
                }
            }
        }
        at = close.saturating_add(1);
    }
    skeleton
        .iter()
        .zip(located.iter().zip(&doc))
        .filter(|(_, (located, doc))| **located || !**doc)
        .map(|(leaf, (located, _))| (leaf, *located))
        .collect()
}

/// One version of a file read against the bodies its items have in it.
fn side(path: &str, text: &str, items: &[(String, Span, bool)]) -> Result<Read, Everything> {
    let bodies: Vec<Span> = outermost(
        items
            .iter()
            .map(|(_, body, measurable)| (*body, *measurable)),
    )
    .into_iter()
    .map(|at| {
        items
            .get(at)
            .map_or(Span { start: 0, end: 0 }, |(_, body, _)| *body)
    })
    .collect();
    let unmeasurable: Vec<Span> = items
        .iter()
        .filter(|(_, _, measurable)| !measurable)
        .map(|(_, body, _)| *body)
        .collect();
    read(path, text, &bodies, &unmeasurable)
}

/// Whether the two skeletons are one skeleton, with what a foreign macro reads where it was.
fn same_skeleton(
    path: &str,
    (old, new): (&Read, &Read),
    shadows: &Shadows,
) -> Result<(), Everything> {
    let was = visible(&old.skeleton, shadows);
    let is = visible(&new.skeleton, shadows);
    if let Some(line) = was
        .iter()
        .zip(&is)
        .find(|(was, is)| was.0.text != is.0.text)
        .map(|(was, _)| was.0.line)
        .or_else(|| (was.len() != is.len()).then(|| was.last().map_or(1, |last| last.0.line)))
    {
        return Err(Everything::Skeleton {
            path: path.to_owned(),
            line,
        });
    }
    match was
        .iter()
        .zip(&is)
        .find(|(was, is)| (was.1 || is.1) && was.0.located() != is.0.located())
    {
        Some((moved, _)) => Err(Everything::Located {
            path: path.to_owned(),
            line: moved.0.line,
        }),
        None => Ok(()),
    }
}

/// The measured items `revision` changed, or why no measured item can hold what changed.
fn revised(measured: &Measured<'_>, revision: &Revision<'_>) -> Result<BTreeSet<u32>, Everything> {
    let path = revision.path;
    let cataloged: Vec<&Item> = measured
        .touched
        .items
        .iter()
        .filter(|item| item.path == path)
        .collect();
    let hidden = Shadows {
        names: BTreeSet::new(),
        all: true,
    };
    let shadows = cataloged
        .first()
        .and_then(|item| measured.shadows.get(&item.package))
        .unwrap_or(&hidden);
    let before: Vec<(String, Span, bool)> = cataloged
        .iter()
        .map(|item| (item.name.clone(), item.body, item.measurable))
        .collect();
    let after: Vec<(String, Span, bool)> =
        crate::instrument::items(path, revision.new.as_bytes(), 0)
            .map_err(|_unparsed| Everything::Unparsed {
                path: path.to_owned(),
            })?
            .into_iter()
            .map(|item| (item.name, item.body, item.measurable))
            .collect();
    let old = side(path, revision.old, &before)?;
    let new = side(path, revision.new, &after)?;
    same_skeleton(path, (&old, &new), shadows)?;
    let unmeasurable = cataloged.iter().filter(|item| !item.measurable);
    for (item, (was, is)) in unmeasurable.zip(old.unmeasured.iter().zip(&new.unmeasured)) {
        if !same_place(was, is) {
            return Err(Everything::Unmeasurable {
                item: item.name.clone(),
            });
        }
    }
    let outer = outermost(
        before
            .iter()
            .map(|(_, body, measurable)| (*body, *measurable)),
    );
    let mut changed = BTreeSet::new();
    for (at, (was, is)) in outer
        .into_iter()
        .zip(old.interiors.iter().zip(&new.interiors))
    {
        if same_place(was, is) {
            continue;
        }
        let Some(item) = cataloged.get(at) else {
            continue;
        };
        if let Some(by) = escaping(was, shadows).or_else(|| escaping(is, shadows)) {
            return Err(Everything::Escapes {
                item: item.name.clone(),
                by,
            });
        }
        changed.insert(item.index);
    }
    Ok(changed)
}

/// The items `changes` falls in, or the first change no measurement can place.
///
/// # Errors
/// [`Everything`] naming the first change that cannot be placed in the body of a measured item.
pub fn changed_items(
    measured: &Measured<'_>,
    changes: &[Changed<'_>],
) -> Result<BTreeSet<u32>, Everything> {
    let mut items = BTreeSet::new();
    for change in changes {
        let path = change.path();
        if builds_everything(path) {
            return Err(Everything::Build {
                path: path.to_owned(),
            });
        }
        let revision = match change {
            Changed::Whole { path } => {
                return Err(Everything::Whole {
                    path: (*path).to_owned(),
                });
            }
            Changed::Unproven { path } => {
                return Err(Everything::Unproven {
                    path: (*path).to_owned(),
                });
            }
            Changed::Revised(revision) => revision,
        };
        if !measured.touched.items.iter().any(|item| item.path == path) {
            return Err(Everything::Unitemized {
                path: path.to_owned(),
            });
        }
        items.extend(revised(measured, revision)?);
    }
    Ok(items)
}

/// What a change decides for every target in `now`, the targets the changed tree holds: a target whose tests entered a changed item runs, as does every target whose reach was not shown to hold or that the measurement does not name; the rest are skipped.
#[must_use]
pub fn decide(
    measured: &Measured<'_>,
    now: &BTreeSet<String>,
    changes: &[Changed<'_>],
) -> Selection {
    let Measured {
        touched, standing, ..
    } = *measured;
    let placed = changed_items(measured, changes);
    let decided = now
        .iter()
        .map(|target| {
            let decided = match (&placed, touched.targets.get(target), standing.get(target)) {
                (Err(everything), _, _) => Decided::Run(Why::Everything(everything.clone())),
                (Ok(_), None, _) | (Ok(_), Some(_), None) => Decided::Run(Why::Unmeasured),
                (
                    Ok(_),
                    Some(_),
                    Some(steadiness @ (Steadiness::Moved(_) | Steadiness::NotMeasured(_))),
                ) => Decided::Run(Why::Unestablished(steadiness.clone())),
                (Ok(items), Some(record), Some(Steadiness::Held)) => {
                    let entered: BTreeSet<u32> = record
                        .entered_by_any()
                        .intersection(items)
                        .copied()
                        .collect();
                    if entered.is_empty() {
                        Decided::Skip
                    } else {
                        Decided::Run(Why::Entered(entered))
                    }
                }
            };
            (target.clone(), decided)
        })
        .collect();
    Selection { decided }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{Changed, Decided, Everything, Measured, Selection, Shadows, Why, decide};
    use crate::touch::{Item, Seen, Steadiness, TargetTouches, Touched, Unmeasured};

    const PATH: &str = "src/lib.rs";

    const MEASURED: &str = "\
/// Adds one.
pub fn alpha(x: u8) -> u8 {
    x + 1
}

pub fn beta(x: u8) -> u8 {
    x * 2
}
";

    /// The measurement of `source`: `enters-alpha` entered only `alpha`, and `enters-beta` only `beta`, and both held.
    fn measured(source: &str) -> (Touched, BTreeMap<String, Steadiness>) {
        let items: Vec<Item> = crate::instrument::items(PATH, source.as_bytes(), 0)
            .expect("the measured file parses")
            .into_iter()
            .map(|body| Item {
                index: body.index,
                package: "demo".to_owned(),
                path: PATH.to_owned(),
                name: body.name,
                span: body.span,
                body: body.body,
                measurable: body.measurable,
            })
            .collect();
        let index = |name: &str| {
            items
                .iter()
                .find(|item| item.name == name)
                .map(|item| item.index)
        };
        let entering = |name: &str| TargetTouches {
            entered: Seen {
                tests: BTreeMap::from([("t".to_owned(), index(name).into_iter().collect())]),
                loose: BTreeSet::new(),
            },
            ..TargetTouches::default()
        };
        let touched = Touched {
            targets: BTreeMap::from([
                ("enters-alpha".to_owned(), entering("alpha")),
                ("enters-beta".to_owned(), entering("beta")),
            ]),
            items,
            ..Touched::default()
        };
        let standing = BTreeMap::from([
            ("enters-alpha".to_owned(), Steadiness::Held),
            ("enters-beta".to_owned(), Steadiness::Held),
        ]);
        (touched, standing)
    }

    fn now() -> BTreeSet<String> {
        BTreeSet::from(["enters-alpha".to_owned(), "enters-beta".to_owned()])
    }

    fn shadows_of(source: &str) -> BTreeMap<String, Shadows> {
        BTreeMap::from([("demo".to_owned(), Shadows::of([source]))])
    }

    fn deciding(
        (touched, standing): &(Touched, BTreeMap<String, Steadiness>),
        shadows: &BTreeMap<String, Shadows>,
        now: &BTreeSet<String>,
        changes: &[Changed<'_>],
    ) -> Selection {
        decide(
            &Measured {
                touched,
                standing,
                shadows,
            },
            now,
            changes,
        )
    }

    fn selected(old: &str, new: &str) -> Selection {
        let digest = crate::id::digest(old.as_bytes());
        deciding(
            &measured(old),
            &shadows_of(old),
            &now(),
            &[Changed::read(PATH, &digest, Some(old), Some(new))],
        )
    }

    fn skipped(selection: &Selection) -> Vec<&str> {
        selection.skippable().collect()
    }

    fn everything(selection: &Selection) -> Option<&Everything> {
        let why: Vec<Option<&Everything>> = selection
            .decided()
            .values()
            .map(|decided| match decided {
                Decided::Run(Why::Everything(everything)) => Some(everything),
                Decided::Skip | Decided::Run(_) => None,
            })
            .collect();
        match why.as_slice() {
            [Some(first), rest @ ..] if rest.iter().all(|one| *one == Some(*first)) => Some(first),
            _ => None,
        }
    }

    #[test]
    fn a_body_edit_runs_the_targets_that_entered_it_and_skips_the_rest() {
        let selection = selected(MEASURED, &MEASURED.replace("x * 2", "x * 3"));
        assert_eq!(skipped(&selection), ["enters-alpha"], "{selection:?}");
        assert!(
            matches!(
                selection.decided().get("enters-beta"),
                Some(Decided::Run(Why::Entered(_)))
            ),
            "{selection:?}"
        );
    }

    #[test]
    fn a_line_inserted_before_a_brace_on_its_own_line_is_a_signature_change() {
        let old = MEASURED.replace("pub fn beta(x: u8) -> u8 {", "pub fn beta(x: u8) -> u8\n{");
        let new = old.replace("-> u8\n{", "-> u8\nwhere u8: Copy,\n{");
        let selection = selected(&old, &new);
        assert!(
            matches!(everything(&selection), Some(Everything::Skeleton { .. })),
            "a `where` between the signature and a brace that opens its own line is read by \
             every caller: {selection:?}"
        );
    }

    #[test]
    fn an_impl_added_inside_a_body_reaches_past_it() {
        let selection = selected(
            MEASURED,
            &MEASURED.replace(
                "    x + 1\n",
                "    impl Drop for Beta { fn drop(&mut self) { panic!() } }\n    x + 1\n",
            ),
        );
        assert!(
            matches!(everything(&selection), Some(Everything::Escapes { by, .. }) if by == "impl"),
            "every target that drops a Beta runs the new drop without entering alpha: \
             {selection:?}"
        );
    }

    #[test]
    fn a_macro_outside_the_standard_set_reaches_past_the_body_it_is_called_in() {
        let selection = selected(
            MEASURED,
            &MEASURED.replace("    x + 1\n", "    define_things!();\n    x + 1\n"),
        );
        assert!(
            matches!(everything(&selection), Some(Everything::Escapes { by, .. }) if by == "define_things!"),
            "{selection:?}"
        );
        let contained = selected(
            MEASURED,
            &MEASURED.replace("    x + 1\n", "    assert!(x < 9);\n    x + 1\n"),
        );
        assert!(
            everything(&contained).is_none(),
            "`assert!` expands to an expression of its arguments: {contained:?}"
        );
    }

    #[test]
    fn an_item_a_line_moved_is_changed_because_its_panics_say_where_they_are() {
        let selection = selected(
            MEASURED,
            &MEASURED.replace("    x + 1\n", "    let y = x;\n    y + 1\n"),
        );
        assert!(
            skipped(&selection).is_empty(),
            "beta moved down a line, so a test holding a `Location` from it sees another: \
             {selection:?}"
        );
    }

    #[test]
    fn a_comment_that_moves_nothing_changes_nothing() {
        let selection = selected(
            MEASURED,
            &MEASURED.replace("    x * 2\n", "    x * 2 // doubled\n"),
        );
        assert_eq!(
            skipped(&selection),
            ["enters-alpha", "enters-beta"],
            "{selection:?}"
        );
    }

    #[test]
    fn a_doc_comment_on_a_plain_item_is_read_by_rustdoc_alone() {
        let selection = selected(
            MEASURED,
            &MEASURED.replace("/// Adds one.", "/// Adds exactly one."),
        );
        assert_eq!(
            skipped(&selection),
            ["enters-alpha", "enters-beta"],
            "the doc target is never measured and so always runs; nothing else reads the \
             text: {selection:?}"
        );
    }

    #[test]
    fn an_item_a_foreign_derive_reads_is_located_where_it_is() {
        let old = format!(
            "#[derive(Debug, serde::Deserialize)]\npub struct Beta {{ a: u8 }}\n\n{MEASURED}"
        );
        let new = old.replacen("pub struct", "\npub struct", 1);
        let selection = selected(&old, &new);
        assert!(
            matches!(
                everything(&selection),
                Some(Everything::Located { line: 2, .. })
            ),
            "what a derive outside the standard set expands to may carry the span of what it \
             read: {selection:?}"
        );
    }

    #[test]
    fn a_const_fn_that_moved_is_unmeasurable() {
        let old = format!("{MEASURED}\npub const fn gamma() -> u8 {{ 7 }}\n");
        let new = old.replace("    x + 1\n", "    let y = x;\n    y + 1\n");
        let selection = selected(&old, &new);
        assert!(
            matches!(everything(&selection), Some(Everything::Unmeasurable { item }) if item == "gamma"),
            "a const fn called at run time panics where it is, and no guard records who \
             called it: {selection:?}"
        );
    }

    #[test]
    fn measured_bytes_that_are_not_the_measured_ones_prove_nothing() {
        let selection = deciding(
            &measured(MEASURED),
            &shadows_of(MEASURED),
            &now(),
            &[Changed::read(PATH, "0000", Some(MEASURED), Some(MEASURED))],
        );
        assert!(
            matches!(everything(&selection), Some(Everything::Unproven { .. })),
            "{selection:?}"
        );
    }

    #[test]
    fn a_build_file_a_whole_file_and_an_unitemized_file_each_run_everything() {
        let measurement = measured(MEASURED);
        for (change, expected) in [
            (
                Changed::read("Cargo.toml", "", None, Some("")),
                Everything::Build {
                    path: "Cargo.toml".to_owned(),
                },
            ),
            (
                Changed::read(PATH, "", None, Some(MEASURED)),
                Everything::Whole {
                    path: PATH.to_owned(),
                },
            ),
            (
                Changed::read(
                    "tests/data.txt",
                    &crate::id::digest(b"a"),
                    Some("a"),
                    Some("b"),
                ),
                Everything::Unitemized {
                    path: "tests/data.txt".to_owned(),
                },
            ),
        ] {
            let selection = deciding(&measurement, &shadows_of(MEASURED), &now(), &[change]);
            assert_eq!(everything(&selection), Some(&expected), "{selection:?}");
        }
    }

    #[test]
    fn every_target_that_exists_now_is_decided_and_only_those() {
        let (touched, mut standing) = measured(MEASURED);
        standing.insert(
            "enters-beta".to_owned(),
            Steadiness::NotMeasured(Unmeasured::OtherTests),
        );
        let now = BTreeSet::from(["enters-beta".to_owned(), "added-since".to_owned()]);
        let digest = crate::id::digest(MEASURED.as_bytes());
        let selection = deciding(
            &(touched, standing),
            &shadows_of(MEASURED),
            &now,
            &[Changed::read(PATH, &digest, Some(MEASURED), Some(MEASURED))],
        );
        assert_eq!(
            selection.decided().keys().collect::<Vec<_>>(),
            ["added-since", "enters-beta"],
            "a target measured but gone is nothing to run, and one added since is decided: \
             {selection:?}"
        );
        assert_eq!(
            selection.decided().get("added-since"),
            Some(&Decided::Run(Why::Unmeasured))
        );
        assert_eq!(
            selection.decided().get("enters-beta"),
            Some(&Decided::Run(Why::Unestablished(Steadiness::NotMeasured(
                Unmeasured::OtherTests
            ))))
        );
    }

    #[test]
    fn a_standard_name_the_package_declares_otherwise_is_not_the_standard_one() {
        let moved = |old: &str| {
            let new = old.replace("    x + 1\n", "    let y = x;\n    y + 1\n");
            selected(old, &new)
        };
        let derived = format!(
            "use derive_more::Debug;\n#[derive(Debug)]\npub struct Gamma {{ a: u8 }}\n{MEASURED}"
        );
        let tested = format!("{MEASURED}use tokio::test;\n#[test]\nfn delta() {{}}\n");
        let standard = format!("{MEASURED}#[derive(Debug)]\npub struct Gamma {{ a: u8 }}\n");
        assert!(
            everything(&moved(&derived)).is_none(),
            "above the move a located item has not moved: {:?}",
            moved(&derived)
        );
        assert!(
            matches!(
                everything(&moved(&tested)),
                Some(Everything::Located { .. })
            ),
            "`#[test]` is tokio's where `tokio::test` is imported, and it reads what it is on: {:?}",
            moved(&tested)
        );
        let below = format!(
            "{MEASURED}use derive_more::Debug;\n#[derive(Debug)]\npub struct Gamma {{ a: u8 }}\n"
        );
        assert!(
            matches!(everything(&moved(&below)), Some(Everything::Located { .. })),
            "`Debug` is derive_more's where it is imported: {:?}",
            moved(&below)
        );
        assert!(
            everything(&moved(&standard)).is_none(),
            "std's `Debug` carries no location: {:?}",
            moved(&standard)
        );
    }

    #[test]
    fn a_macro_the_package_defines_under_a_standard_name_is_not_contained() {
        let old = format!("macro_rules! vec {{ ($($t:tt)*) => {{ () }} }}\n{MEASURED}");
        let new = old.replace("    x + 1\n", "    vec![];\n    x + 1\n");
        assert!(
            matches!(everything(&selected(&old, &new)), Some(Everything::Escapes { by, .. }) if by == "vec!"),
            "{:?}",
            selected(&old, &new)
        );
        let used = format!("#[macro_use]\nextern crate helpers;\n{MEASURED}");
        let new = used.replace("    x + 1\n", "    assert!(x > 0);\n    x + 1\n");
        assert!(
            matches!(everything(&selected(&used, &new)), Some(Everything::Escapes { by, .. }) if by == "assert!"),
            "every macro a `#[macro_use] extern crate` imports may be named `assert`: {:?}",
            selected(&used, &new)
        );
        let globbed = format!("use helpers::prelude::*;\n{MEASURED}");
        let new = globbed.replace("    x + 1\n", "    assert!(x > 0);\n    x + 1\n");
        assert!(
            matches!(
                everything(&selected(&globbed, &new)),
                Some(Everything::Escapes { .. })
            ),
            "a glob from outside the package outranks the prelude: {:?}",
            selected(&globbed, &new)
        );
        let local = MEASURED.replace(
            "    x + 1\n",
            "    use helpers::vec;\n    vec![];\n    x + 1\n",
        );
        assert!(
            matches!(
                everything(&selected(MEASURED, &local)),
                Some(Everything::Escapes { .. })
            ),
            "a `use` inside a changed body can shadow what the body calls: {:?}",
            selected(MEASURED, &local)
        );
    }
}
