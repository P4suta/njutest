// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which targets a change can be noticed by, decided from the items each target entered on a measured tree.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

use crate::span::Span;
use crate::touch::{Item, Steadiness, Touched, Unmeasured};

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
    /// The change to `path`, given the digests the measurement and the survey now recorded for it and its bytes then and now: bytes that are not the ones digested prove nothing about either.
    #[must_use]
    pub fn read(
        path: &'a str,
        (measured, now): (&crate::id::HexDigest, &crate::id::HexDigest),
        old: Option<&'a str>,
        new: Option<&'a str>,
    ) -> Self {
        match (old, new) {
            (Some(old), Some(new))
                if crate::id::HexDigest::of(old.as_bytes()) == *measured
                    && crate::id::HexDigest::of(new.as_bytes()) == *now =>
            {
                Self::Revised(Revision { path, old, new })
            }
            (Some(_) | None, Some(_) | None) => Self::Unproven { path },
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
    /// The toolchain is not the one that built the measured tree.
    Toolchain {
        /// The one that did.
        measured: String,
        /// The one there is now.
        now: String,
    },
    /// The tree is read by other rules than the measured one was, so a file can appear or vanish with nobody having edited it.
    Rules,
    /// Something the caller says a run is compiled or started with differs: the build selection, the harness arguments.
    Setting {
        /// What differs.
        name: String,
    },
    /// A build script watches something that changed: a file it named, a variable it named, or, where it named none, any file of its package.
    BuildScript {
        /// The package's directory in the tree.
        package: String,
    },
    /// A variable of the environment the run selects has another value, or is set on one side only.
    Environment {
        /// The variable.
        name: String,
    },
    /// A variable the compiler read through `env!` or `option_env!` has another value now.
    Compiled {
        /// The variable.
        name: String,
    },
    /// A file the build read outside the tree changed or is gone.
    Outside {
        /// Its absolute path.
        path: String,
    },
    /// An entry that is not a regular file appeared, vanished, or points elsewhere.
    Irregular {
        /// The entry.
        path: String,
    },
    /// A file compiled into code that runs while the build does changed: a procedural macro or a build script decides what other targets compile to.
    CompileTime {
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

/// What a build read that no survey of the tree sees.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    /// Every file outside the tree and outside the build's own output the compiler read, by absolute path, with its SHA-256: a path dependency, a `[patch]` source, a registry crate.
    pub outside: BTreeMap<String, crate::id::HexDigest>,
    /// Every variable the compiler read through `env!` or `option_env!`, with the value it read or nothing where it was unset.
    pub env: BTreeMap<String, Option<String>>,
    /// Every file of the tree compiled into code that runs while the build does — a procedural macro, a build script — by its `/`-normalized path: no test entering it or not says what an edit to it changes.
    pub compile_time: BTreeSet<String>,
    /// Every variable a build script put in the compiler's environment, which the script decides and no environment of a selection holds.
    pub scripted: BTreeSet<String>,
    /// What each package's build script said it depends on, by the package's `/`-normalized directory in the tree.
    pub scripts: BTreeMap<String, Script>,
}

/// What one build script said, through `rerun-if-changed` and `rerun-if-env-changed`, its output depends on: cargo's own contract for when to run it again.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// The files it watches.
    pub watched: Watched,
    /// Every variable it watches, with the value the measured build ran it under or nothing where it was unset.
    pub env: BTreeMap<String, Option<String>>,
}

/// The files one build script watches.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Watched {
    /// It named none, so cargo runs it again for any change to its package.
    Package,
    /// It named these.
    Paths {
        /// Every path inside the tree it named, file or directory, `/`-normalized.
        inside: BTreeSet<String>,
        /// Every path outside the tree it named, by absolute path, with the digest of what is there.
        outside: BTreeMap<String, crate::id::HexDigest>,
    },
}

/// The digest of what is at `path`: a file's bytes, every file under a directory with its place, or the fact that nothing is there.
#[must_use]
pub fn fingerprint(path: &std::path::Path) -> crate::id::HexDigest {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(next) = pending.pop() {
        hasher.update(next.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.is_dir() => {
                let listing = match std::fs::read_dir(&next) {
                    Ok(listing) => listing,
                    Err(_unreadable) => {
                        hasher.update(b"unreadable directory\0");
                        continue;
                    }
                };
                let mut children = Vec::new();
                for entry in listing {
                    match entry {
                        Ok(entry) => children.push(entry.path()),
                        Err(_unlistable) => hasher.update(b"unlistable entry\0"),
                    }
                }
                children.sort();
                children.reverse();
                pending.extend(children);
            }
            Ok(_) => match std::fs::read(&next) {
                Ok(bytes) => hasher.update(crate::id::HexDigest::of(&bytes).as_str().as_bytes()),
                Err(_unreadable) => hasher.update(b"unreadable file"),
            },
            Err(_absent) => hasher.update(b"absent"),
        }
        hasher.update(b"\0");
    }
    crate::id::HexDigest::finish(hasher)
}

/// Whether one target's reach was shown to be a function of the target, as a measurement records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Standing {
    /// A second whole run passed the same tests and reached, entered, and infected the same.
    Held,
    /// A second whole run reached something else.
    Moved,
    /// A second whole run established nothing to compare, and why.
    NotMeasured {
        /// Why.
        why: Unmeasured,
    },
    /// No second whole run of it was made.
    Uncompared,
    /// A run of it without the tree's source files answered differently, so its tests read the tree as data and an edit it never entered can still move them.
    ReadsTree,
}

/// Every standing but the one a selection may skip by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unheld {
    /// A second whole run reached something else.
    Moved,
    /// A second whole run established nothing to compare, and why.
    NotMeasured {
        /// Why.
        why: Unmeasured,
    },
    /// No second whole run of it was made.
    Uncompared,
    /// A run of it without the tree's source files answered differently, so its tests read the tree as data and an edit it never entered can still move them.
    ReadsTree,
}

impl Standing {
    /// What a control's comparison comes to, as a measurement records it.
    #[must_use]
    pub const fn of(steadiness: &Steadiness) -> Self {
        match steadiness {
            Steadiness::Held => Self::Held,
            Steadiness::Moved(_) => Self::Moved,
            Steadiness::NotMeasured(why) => Self::NotMeasured { why: *why },
        }
    }
}

/// One target as a measurement found it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Every item anything of the target entered.
    pub entered: BTreeSet<u32>,
    /// Whether that was shown to be a function of the target.
    pub standing: Standing,
}

/// What one measured tree establishes for a selection: every file it held, what its build read beyond them, its items, and what each target entered.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    /// The toolchain, as it names itself.
    pub toolchain: String,
    /// Every file of the tree, and the rules it was read by.
    pub survey: crate::snapshot::Survey,
    /// What the build read that the survey does not see.
    pub inputs: Inputs,
    /// The environment the run selected, which every test process ran with.
    pub environment: BTreeMap<String, String>,
    /// What the caller compiled and started the tree with, by name: the build selection, the harness arguments.
    pub settings: BTreeMap<String, String>,
    /// Every item, by item index.
    pub items: Vec<Item>,
    /// Every target the baseline recorded.
    pub targets: BTreeMap<String, Target>,
    /// What each package declares over the standard names.
    pub shadows: BTreeMap<String, Shadows>,
}

/// What [`Measurement::of`] is made from: what one tree was, and what running it established.
#[derive(Debug, Clone, Copy)]
pub struct Parts<'a> {
    /// The toolchain, as it names itself.
    pub toolchain: &'a str,
    /// Every file of the tree.
    pub survey: &'a crate::snapshot::Survey,
    /// What the build read beyond the tree.
    pub inputs: &'a Inputs,
    /// The environment the run selected.
    pub environment: &'a BTreeMap<String, String>,
    /// What the tree was compiled and started with.
    pub settings: &'a BTreeMap<String, String>,
    /// What the baseline recorded.
    pub touched: &'a Touched,
    /// What a second run of each target established.
    pub standing: &'a BTreeMap<String, Steadiness>,
    /// The targets a run without the tree's source files answered differently for.
    pub reading: &'a BTreeSet<String>,
}

impl Measurement {
    /// The measurement `parts` make, with the shadows read from `sources`: every source file of the tree, as the package that compiled it and its text.
    #[must_use]
    pub fn of<'a>(parts: Parts<'_>, sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut texts: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (package, text) in sources {
            texts.entry(package).or_default().push(text);
        }
        Self {
            toolchain: parts.toolchain.to_owned(),
            survey: parts.survey.clone(),
            inputs: parts.inputs.clone(),
            environment: parts.environment.clone(),
            settings: parts.settings.clone(),
            items: parts.touched.items.clone(),
            targets: parts
                .touched
                .targets
                .iter()
                .map(|(target, record)| {
                    let standing = if parts.reading.contains(target) {
                        Standing::ReadsTree
                    } else {
                        parts
                            .standing
                            .get(target)
                            .map_or(Standing::Uncompared, Standing::of)
                    };
                    (
                        target.clone(),
                        Target {
                            entered: record.entered_by_any(),
                            standing,
                        },
                    )
                })
                .collect(),
            shadows: texts
                .into_iter()
                .map(|(package, texts)| (package.to_owned(), Shadows::of(texts)))
                .collect(),
        }
    }
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
    Unestablished(Unheld),
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

    /// Every target of `now` running for the one reason `why`.
    #[must_use]
    pub fn everything(now: &BTreeSet<String>, why: &Everything) -> Self {
        Self {
            decided: now
                .iter()
                .map(|target| (target.clone(), Decided::Run(Why::Everything(why.clone()))))
                .collect(),
        }
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
            | ".njutest.toml"
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
fn revised(measured: &Measurement, revision: &Revision<'_>) -> Result<BTreeSet<u32>, Everything> {
    let path = revision.path;
    let cataloged: Vec<&Item> = measured
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
    measured: &Measurement,
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
        if measured.inputs.compile_time.contains(path) {
            return Err(Everything::CompileTime {
                path: path.to_owned(),
            });
        }
        if !measured.items.iter().any(|item| item.path == path) {
            return Err(Everything::Unitemized {
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
        items.extend(revised(measured, revision)?);
    }
    Ok(items)
}

/// What the tree and its build are now, as a selection holds them against a measurement.
#[derive(Debug, Clone, Copy)]
pub struct Now<'a> {
    /// The toolchain, as it names itself.
    pub toolchain: &'a str,
    /// Every file of the tree, read by the rules a run would copy it by.
    pub survey: &'a crate::snapshot::Survey,
    /// The environment a run would select.
    pub environment: &'a BTreeMap<String, String>,
    /// What a run would compile and start the tree with.
    pub settings: &'a BTreeMap<String, String>,
    /// Every variable a build would see, which is where a variable the compiler read is looked up.
    pub vars: &'a BTreeMap<String, String>,
}

/// One file whose bytes, mode, or presence differ from the measured tree's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// The file is in both, with other bytes or another mode.
    Edited {
        /// The file.
        path: String,
        /// What the measurement recorded of it.
        measured: crate::snapshot::Surveyed,
        /// What the survey now recorded of it.
        now: crate::snapshot::Surveyed,
    },
    /// The file is new, or gone.
    Whole {
        /// The file.
        path: String,
    },
}

/// The first name the two maps disagree about, set on one side only or to two values.
fn unequal(was: &BTreeMap<String, String>, is: &BTreeMap<String, String>) -> Option<String> {
    was.keys()
        .chain(is.keys())
        .find(|name| was.get(*name) != is.get(*name))
        .cloned()
}

/// Whether a variable is one cargo sets for a compilation, which is a function of the manifests and the paths it builds in rather than of anything a person sets.
fn set_by_cargo(name: &str) -> bool {
    name.starts_with("CARGO_") || name == "OUT_DIR"
}

/// The files that differ between the measured tree and the tree `now`, or the first difference in what the build read that no file of the tree names.
///
/// # Errors
/// [`Everything`] naming a moved toolchain, walk rule, selected variable, compiled variable, outside file, or irregular entry.
pub fn differences(measured: &Measurement, now: &Now<'_>) -> Result<Vec<Difference>, Everything> {
    if measured.toolchain != now.toolchain {
        return Err(Everything::Toolchain {
            measured: measured.toolchain.clone(),
            now: now.toolchain.to_owned(),
        });
    }
    if measured.survey.rules != now.survey.rules {
        return Err(Everything::Rules);
    }
    if let Some(name) = unequal(&measured.settings, now.settings) {
        return Err(Everything::Setting { name });
    }
    if let Some(name) = unequal(&measured.environment, now.environment) {
        return Err(Everything::Environment { name });
    }
    if let Some((name, _)) = measured
        .inputs
        .env
        .iter()
        .filter(|(name, _)| !set_by_cargo(name) && !measured.inputs.scripted.contains(*name))
        .find(|(name, value)| now.vars.get(*name) != value.as_ref())
    {
        return Err(Everything::Compiled { name: name.clone() });
    }
    for (path, digest) in &measured.inputs.outside {
        let unchanged = match std::fs::read(path) {
            Ok(bytes) => crate::id::HexDigest::of(&bytes) == *digest,
            Err(_gone_or_unreadable) => false,
        };
        if !unchanged {
            return Err(Everything::Outside { path: path.clone() });
        }
    }
    let irregular: BTreeSet<&String> = measured
        .survey
        .passed_over
        .keys()
        .chain(now.survey.passed_over.keys())
        .collect();
    if let Some(path) = irregular
        .into_iter()
        .find(|path| measured.survey.passed_over.get(*path) != now.survey.passed_over.get(*path))
    {
        return Err(Everything::Irregular { path: path.clone() });
    }
    let paths: BTreeSet<&String> = measured
        .survey
        .files
        .keys()
        .chain(now.survey.files.keys())
        .collect();
    let found: Vec<Difference> = paths
        .into_iter()
        .filter_map(
            |path| match (measured.survey.files.get(path), now.survey.files.get(path)) {
                (Some(was), Some(is)) if was == is => None,
                (Some(was), Some(is)) => Some(Difference::Edited {
                    path: path.clone(),
                    measured: was.clone(),
                    now: is.clone(),
                }),
                (None, _) | (_, None) => Some(Difference::Whole { path: path.clone() }),
            },
        )
        .collect();
    for (package, script) in &measured.inputs.scripts {
        let moved = scripted(package, script, &found, now.vars);
        if moved {
            return Err(Everything::BuildScript {
                package: package.clone(),
            });
        }
    }
    Ok(found)
}

/// Whether what `script` watches moved between the measured tree and the tree whose differences are `found`.
fn scripted(
    package: &str,
    script: &Script,
    found: &[Difference],
    vars: &BTreeMap<String, String>,
) -> bool {
    let within = |path: &str, directory: &str| {
        directory.is_empty() || path == directory || path.starts_with(&format!("{directory}/"))
    };
    let changed = |path: &str| {
        found.iter().any(|difference| match difference {
            Difference::Edited { path: edited, .. } | Difference::Whole { path: edited } => {
                within(edited, path)
            }
        })
    };
    let env_moved = script
        .env
        .iter()
        .any(|(name, value)| vars.get(name) != value.as_ref());
    env_moved
        || match &script.watched {
            Watched::Package => changed(package),
            Watched::Paths { inside, outside } => {
                inside.iter().any(|path| changed(path))
                    || outside
                        .iter()
                        .any(|(path, digest)| fingerprint(std::path::Path::new(path)) != *digest)
            }
        }
}

/// What a change decides for every target in `now`, the targets the changed tree holds: a target whose tests entered a changed item runs, as does every target whose reach was not shown to hold or that the measurement does not name; the rest are skipped.
#[must_use]
pub fn decide(
    measured: &Measurement,
    now: &BTreeSet<String>,
    changes: &[Changed<'_>],
) -> Selection {
    let placed = changed_items(measured, changes);
    let decided = now
        .iter()
        .map(|target| {
            let decided = match (&placed, measured.targets.get(target)) {
                (Err(everything), _) => Decided::Run(Why::Everything(everything.clone())),
                (Ok(_), None) => Decided::Run(Why::Unmeasured),
                (Ok(items), Some(record)) => match record.standing {
                    Standing::Moved => Decided::Run(Why::Unestablished(Unheld::Moved)),
                    Standing::NotMeasured { why } => {
                        Decided::Run(Why::Unestablished(Unheld::NotMeasured { why }))
                    }
                    Standing::Uncompared => Decided::Run(Why::Unestablished(Unheld::Uncompared)),
                    Standing::ReadsTree => Decided::Run(Why::Unestablished(Unheld::ReadsTree)),
                    Standing::Held => {
                        let entered: BTreeSet<u32> =
                            record.entered.intersection(items).copied().collect();
                        if entered.is_empty() {
                            Decided::Skip
                        } else {
                            Decided::Run(Why::Entered(entered))
                        }
                    }
                },
            };
            (target.clone(), decided)
        })
        .collect();
    Selection { decided }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        Changed, Decided, Difference, Everything, Inputs, Measurement, Now, Parts, Selection,
        Standing, Unheld, Why, decide, differences,
    };
    use crate::id::HexDigest;
    use crate::snapshot::{Survey, Surveyed};
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
    fn measured(source: &str) -> Measurement {
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
        Measurement::of(
            Parts {
                toolchain: "rustc",
                survey: &surveyed(&[(PATH, source)]),
                inputs: &Inputs::default(),
                environment: &BTreeMap::new(),
                settings: &BTreeMap::from([("build".to_owned(), "default".to_owned())]),
                touched: &touched,
                standing: &standing,
                reading: &BTreeSet::new(),
            },
            [("demo", source)],
        )
    }

    fn surveyed(files: &[(&str, &str)]) -> Survey {
        Survey {
            rules: "rules".to_owned(),
            files: files
                .iter()
                .map(|(path, text)| {
                    (
                        (*path).to_owned(),
                        Surveyed {
                            sha256: HexDigest::of(text.as_bytes()),
                            executable: false,
                        },
                    )
                })
                .collect(),
            passed_over: BTreeMap::new(),
        }
    }

    fn now() -> BTreeSet<String> {
        BTreeSet::from(["enters-alpha".to_owned(), "enters-beta".to_owned()])
    }

    fn selected(old: &str, new: &str) -> Selection {
        let (was, is) = (HexDigest::of(old.as_bytes()), HexDigest::of(new.as_bytes()));
        decide(
            &measured(old),
            &now(),
            &[Changed::read(PATH, (&was, &is), Some(old), Some(new))],
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
        let selection = decide(
            &measured(MEASURED),
            &now(),
            &[Changed::read(
                PATH,
                (
                    &HexDigest::of(b"other"),
                    &HexDigest::of(MEASURED.as_bytes()),
                ),
                Some(MEASURED),
                Some(MEASURED),
            )],
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
                Changed::Whole { path: "Cargo.toml" },
                Everything::Build {
                    path: "Cargo.toml".to_owned(),
                },
            ),
            (
                Changed::Whole { path: PATH },
                Everything::Whole {
                    path: PATH.to_owned(),
                },
            ),
            (
                Changed::read(
                    "tests/data.txt",
                    (&HexDigest::of(b"a"), &HexDigest::of(b"b")),
                    Some("a"),
                    Some("b"),
                ),
                Everything::Unitemized {
                    path: "tests/data.txt".to_owned(),
                },
            ),
        ] {
            let selection = decide(&measurement, &now(), &[change]);
            assert_eq!(everything(&selection), Some(&expected), "{selection:?}");
        }
    }

    #[test]
    fn every_target_that_exists_now_is_decided_and_only_those() {
        let mut measurement = measured(MEASURED);
        if let Some(beta) = measurement.targets.get_mut("enters-beta") {
            beta.standing = Standing::NotMeasured {
                why: Unmeasured::OtherTests,
            };
        }
        let now = BTreeSet::from(["enters-beta".to_owned(), "added-since".to_owned()]);
        let digest = HexDigest::of(MEASURED.as_bytes());
        let selection = decide(
            &measurement,
            &now,
            &[Changed::read(
                PATH,
                (&digest, &digest),
                Some(MEASURED),
                Some(MEASURED),
            )],
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
            Some(&Decided::Run(Why::Unestablished(Unheld::NotMeasured {
                why: Unmeasured::OtherTests
            })))
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

    fn found(
        measured: &Measurement,
        survey: &Survey,
        vars: &[(&str, &str)],
    ) -> Result<Vec<Difference>, Everything> {
        let vars: BTreeMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        differences(
            measured,
            &Now {
                toolchain: "rustc",
                survey,
                environment: &BTreeMap::new(),
                settings: &BTreeMap::from([("build".to_owned(), "default".to_owned())]),
                vars: &vars,
            },
        )
    }

    #[test]
    fn the_files_that_differ_are_every_edit_every_mode_change_and_every_file_added_or_gone() {
        let mut measured = measured(MEASURED);
        measured.survey = surveyed(&[(PATH, MEASURED), ("run.sh", "x"), ("gone.txt", "y")]);
        let mut survey = surveyed(&[(PATH, "edited"), ("run.sh", "x"), ("new.txt", "z")]);
        if let Some(script) = survey.files.get_mut("run.sh") {
            script.executable = true;
        }
        let named: Vec<(bool, String)> = found(&measured, &survey, &[])
            .expect("nothing but files moved")
            .into_iter()
            .map(|difference| match difference {
                Difference::Whole { path } => (true, path),
                Difference::Edited { path, .. } => (false, path),
            })
            .collect();
        assert_eq!(
            named,
            [
                (true, "gone.txt".to_owned()),
                (true, "new.txt".to_owned()),
                (false, "run.sh".to_owned()),
                (false, PATH.to_owned()),
            ],
            "a file made runnable with the same bytes changed for a test that runs it"
        );
        assert_eq!(
            found(&measured, &measured.survey.clone(), &[]),
            Ok(Vec::new())
        );
    }

    #[test]
    fn what_the_build_read_beyond_the_tree_is_held_to_the_measurement() {
        let measured = measured(MEASURED);
        let same = measured.survey.clone();
        let mut toolchain = measured.clone();
        toolchain.toolchain = "rustc 1.0".to_owned();
        assert!(matches!(
            found(&toolchain, &same, &[]),
            Err(Everything::Toolchain { .. })
        ));
        let mut built = measured.clone();
        built
            .settings
            .insert("build".to_owned(), "imperial".to_owned());
        assert_eq!(
            found(&built, &same, &[]),
            Err(Everything::Setting {
                name: "build".to_owned()
            }),
            "another build selection compiles another program"
        );
        let mut rules = same.clone();
        rules.rules = "other".to_owned();
        assert_eq!(found(&measured, &rules, &[]), Err(Everything::Rules));
        let mut selected = measured.clone();
        selected
            .environment
            .insert("LANG".to_owned(), "C".to_owned());
        assert_eq!(
            found(&selected, &same, &[]),
            Err(Everything::Environment {
                name: "LANG".to_owned()
            }),
            "a variable the run selects set on one side only"
        );
        let mut compiled = measured.clone();
        compiled
            .inputs
            .env
            .insert("GREETING".to_owned(), Some("hello".to_owned()));
        compiled
            .inputs
            .env
            .insert("CARGO_MANIFEST_DIR".to_owned(), Some("/copy".to_owned()));
        assert_eq!(
            found(&compiled, &same, &[("GREETING", "hello")]),
            Ok(Vec::new()),
            "cargo's own variables follow the manifests and the copy's path"
        );
        assert_eq!(
            found(&compiled, &same, &[("GREETING", "bye")]),
            Err(Everything::Compiled {
                name: "GREETING".to_owned()
            })
        );
        let directory = tempfile::tempdir().expect("tempdir");
        let dependency = directory.path().join("dep.rs");
        std::fs::write(&dependency, "pub fn f() {}").expect("write");
        let mut outside = measured.clone();
        let name = dependency.display().to_string();
        outside
            .inputs
            .outside
            .insert(name.clone(), HexDigest::of(b"pub fn f() {}"));
        assert_eq!(found(&outside, &same, &[]), Ok(Vec::new()));
        std::fs::write(&dependency, "pub fn f() { g() }").expect("write");
        assert_eq!(
            found(&outside, &same, &[]),
            Err(Everything::Outside { path: name })
        );
        let mut linked = same;
        linked
            .passed_over
            .insert("fixtures/current".to_owned(), Some("v2".to_owned()));
        assert!(matches!(
            found(&measured, &linked, &[]),
            Err(Everything::Irregular { .. })
        ));
    }

    #[test]
    fn bytes_that_are_not_the_ones_surveyed_now_prove_nothing_either() {
        let edited = MEASURED.replace("x * 2", "x * 3");
        let selection = decide(
            &measured(MEASURED),
            &now(),
            &[Changed::read(
                PATH,
                (
                    &HexDigest::of(MEASURED.as_bytes()),
                    &HexDigest::of(edited.as_bytes()),
                ),
                Some(MEASURED),
                Some(&MEASURED.replace("x * 2", "x * 4")),
            )],
        );
        assert!(
            matches!(everything(&selection), Some(Everything::Unproven { .. })),
            "a file edited again after the survey read it is not the file the survey saw: \
             {selection:?}"
        );
    }

    #[test]
    fn a_build_script_moves_everything_when_what_it_watches_moves() {
        use super::{Script, Watched};
        let mut measured = measured(MEASURED);
        measured.survey = surveyed(&[(PATH, MEASURED), ("data/answer.txt", "42")]);
        let edited = surveyed(&[(PATH, MEASURED), ("data/answer.txt", "43")]);
        let watching = |watched: Watched| {
            let mut one = measured.clone();
            one.inputs.scripts = BTreeMap::from([(
                String::new(),
                Script {
                    watched,
                    env: BTreeMap::new(),
                },
            )]);
            one
        };
        let named = watching(Watched::Paths {
            inside: BTreeSet::from(["data".to_owned()]),
            outside: BTreeMap::new(),
        });
        assert_eq!(
            found(&named, &edited, &[]),
            Err(Everything::BuildScript {
                package: String::new()
            }),
            "a file under a directory the script named changed"
        );
        let elsewhere = watching(Watched::Paths {
            inside: BTreeSet::from(["assets".to_owned()]),
            outside: BTreeMap::new(),
        });
        assert!(
            matches!(found(&elsewhere, &edited, &[]), Ok(found) if found.len() == 1),
            "a script that named only `assets` does not run again for `data`"
        );
        let package = watching(Watched::Package);
        assert!(
            matches!(
                found(&package, &edited, &[]),
                Err(Everything::BuildScript { .. })
            ),
            "a script that named nothing runs again for any change to its package"
        );
        let mut variable = watching(Watched::Paths {
            inside: BTreeSet::new(),
            outside: BTreeMap::new(),
        });
        if let Some(script) = variable.inputs.scripts.get_mut("") {
            script.env.insert("WANTED".to_owned(), Some("a".to_owned()));
        }
        assert!(
            found(&variable, &measured.survey.clone(), &[("WANTED", "a")]) == Ok(Vec::new())
                && matches!(
                    found(&variable, &measured.survey.clone(), &[("WANTED", "b")]),
                    Err(Everything::BuildScript { .. })
                ),
            "a variable the script watches changed"
        );
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join("schema.json");
        std::fs::write(&file, "{}").expect("write");
        let outside = watching(Watched::Paths {
            inside: BTreeSet::new(),
            outside: BTreeMap::from([(file.display().to_string(), super::fingerprint(&file))]),
        });
        assert_eq!(
            found(&outside, &measured.survey.clone(), &[]),
            Ok(Vec::new())
        );
        std::fs::write(&file, "{\"a\": 1}").expect("write");
        assert!(
            matches!(
                found(&outside, &measured.survey.clone(), &[]),
                Err(Everything::BuildScript { .. })
            ),
            "a file outside the tree the script watches changed"
        );
    }
}
