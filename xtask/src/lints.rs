// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three things this repository does not write.

use std::fmt;

use syn::visit::Visit;

/// What kind of thing was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Kind {
    /// An `#[allow(…)]` or `#![allow(…)]` attribute.
    AllowAttribute,
    /// A `Box<dyn Trait>` type.
    BoxedTraitObject,
    /// A comment that is not documentation.
    Comment,
    /// A recursive removal outside the one place that bounds it and says what is left.
    UnboundedRemoval,
    /// A command or a configuration key built with an identity in it, which the next edit re-mints.
    PerishableHandle,
    /// An exported constant that spells a directory structure rather than one name.
    LooseLayout,
}

impl Kind {
    /// Every kind, in the order a report lists them.
    pub const ALL: [Self; 6] = [
        Self::AllowAttribute,
        Self::BoxedTraitObject,
        Self::Comment,
        Self::UnboundedRemoval,
        Self::PerishableHandle,
        Self::LooseLayout,
    ];

    /// What to write in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AllowAttribute => "allow-attribute",
            Self::BoxedTraitObject => "boxed-trait-object",
            Self::Comment => "comment",
            Self::UnboundedRemoval => "unbounded-removal",
            Self::PerishableHandle => "perishable-handle",
            Self::LooseLayout => "loose-layout",
        }
    }

    /// Why it is refused, in the words a person needs to fix it.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::AllowAttribute => {
                "use #[expect(…, reason = \"…\")], which the compiler retires when the lint \
                 stops firing"
            }
            Self::BoxedTraitObject => {
                "use an enum for a closed set of implementations, or a generic parameter for \
                 an open one"
            }
            Self::Comment => {
                "say it in the name, in the item's own documentation, or in the message the \
                 assertion prints; a comment beside code is a second account of it that \
                 nothing keeps true"
            }
            Self::UnboundedRemoval => {
                "use rust_mutants::reclaim, which stops at a budget and hands back what \
                 refused and what it never reached; a directory something else is holding \
                 takes minutes to refuse, and a loop over a few hundred of those runs for \
                 a day while saying nothing"
            }
            Self::PerishableHandle => {
                "build it from a locator — path, item, rule — which holds after the file \
                 has changed; a mutant identity is a function of the whole file, so the \
                 edit that closes a survivor re-mints it and the command or the record \
                 that names it stops naming anything"
            }
            Self::LooseLayout => {
                "a layout written down here freezes it: the configuration cannot name a \
                 directory somebody else has already decided, which is how a report \
                 directory stayed unconfigurable while four commands read the wrong \
                 place. Ask the type that owns the layout for the path, the way the code \
                 under test does, and let the default live in the configuration alone"
            }
        }
    }
}

/// What a reader is told to type back at the tool, where an identity in it would not survive them typing it.
const HANDED_OUT: [&str; 5] = [
    "--mutant ",
    "njutest accept ",
    "njutest replay ",
    "rust-mutants explain ",
    "njutest explain ",
];

/// The names of the things that are an identity rather than a place.
const PERISHABLE: [&str; 2] = ["display_id", ".id"];

/// The call this repository does not write directly, because every place that did lost what it could not remove.
const RAW_REMOVAL: &str = "remove_dir_all";

/// The module that is allowed to make it in a loop, being the one that bounds it.
///
/// A test may make it too: what a test removes is what it made, and it is
/// standing there watching.
const RECLAIMER: &str = "crates/rust-mutants/src/reclaim.rs";

/// One thing found in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// What it is.
    pub kind: Kind,
    /// The file, as a repository-relative path.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.file,
            self.line,
            self.kind.label(),
            self.kind.remedy()
        )
    }
}

/// Everything `source` holds that this repository does not write.
///
/// # Errors
/// A file that is not Rust this version can parse.
pub fn scan_source(file: &str, source: &str) -> Result<Vec<Finding>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut scan = Scan {
        file: file.to_owned(),
        found: Vec::new(),
        looping: 0,
        reclaimer: file.ends_with(RECLAIMER) || file.contains("/tests/"),
    };
    scan.visit_file(&parsed);
    scan.found.extend(comments(file, source));
    scan.found.extend(handles(file, source));
    scan.found.sort();
    Ok(scan.found)
}

/// Every exported `&str` constant of `source`, by name, with the line it is on.
///
/// The cross-file pass needs these because what makes a constant a layout is
/// not how it is spelled — `"rust-mutants/explain"` is a document type and
/// `"reports/runs"` is a structure, and they look the same — but that more
/// than one module joins it onto a path.
#[must_use]
pub fn exported_strings(source: &str) -> Vec<(String, usize)> {
    declared(source)
        .filter(|(_at, _name, value)| value.contains('/'))
        .map(|(at, name, _value)| (name.to_owned(), at.saturating_add(1)))
        .collect()
}

/// Every `&str` constant a file declares, whatever its visibility, as line, name and value.
fn declared(source: &str) -> impl Iterator<Item = (usize, &str, &str)> {
    source.lines().enumerate().filter_map(|(at, line)| {
        let rest = line.trim_start();
        let rest = rest
            .split_once("const ")
            .filter(|(before, _rest)| before.is_empty() || before.starts_with("pub"))
            .map(|(_before, rest)| rest)?;
        let (name, value) = rest.split_once(": &str = ")?;
        Some((
            at,
            name.trim(),
            value.trim().trim_matches(|it| it == ';' || it == '"'),
        ))
    })
}

/// The first path segment of every directory the configuration is allowed to move.
///
/// A default a configuration field falls back to is a directory somebody can
/// rename, so a test that writes it down decides it for them. Reading the
/// defaults rather than a list here means a directory added later is gated
/// the day its default is written.
#[must_use]
pub fn configured_directories(source: &str) -> Vec<String> {
    declared(source)
        .filter(|(_at, name, _value)| {
            name.starts_with("DEFAULT_") && (name.ends_with("_DIRECTORY") || name.ends_with("_DIR"))
        })
        .filter_map(|(_at, _name, value)| value.split('/').next())
        .filter(|head| !head.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every line of `source` that spells one of `directories` as the head of a path literal.
///
/// A directory the configuration can move is one no file may write down. The
/// literal is what makes it immovable, whether it is joined onto a root, asked
/// to exist, or handed to the engine as the place this tool writes.
#[must_use]
pub fn spelled(source: &str, directories: &[String]) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, line) in source.lines().enumerate() {
        let start = line.trim_start();
        if start.starts_with("///") || start.starts_with("//!") || start.starts_with("//") {
            continue;
        }
        for literal in literals(line) {
            let structure = literal.contains('/')
                && directories
                    .iter()
                    .any(|head| literal.split('/').next() == Some(head.as_str()));
            let joined = directories.iter().any(|head| head == literal)
                && line.contains(&format!(".join(\"{literal}\")"));
            if structure || joined {
                found.push(at.saturating_add(1));
                break;
            }
        }
    }
    found
}

/// Every double-quoted literal on one line, which is close enough for a line of Rust that holds no escaped quote.
fn literals(line: &str) -> Vec<&str> {
    line.split('"')
        .skip(1)
        .step_by(2)
        .filter(|it| !it.is_empty())
        .collect()
}

/// Whether `source` joins `name` onto a path, which is what makes holding it a layout decision.
#[must_use]
pub fn joins(source: &str, name: &str) -> bool {
    source.contains(&format!(".join({name})"))
        || source.contains(&format!("{{{name}}}/"))
        || source.contains(&format!(".join(&{name})"))
}

/// Which module `source` imports `name` from, when it imports it by name.
///
/// A bare `FILE_NAME` is four different constants in this tree, and only one
/// of them spells a structure. Reading the import is what tells them apart,
/// and a name nothing imports is one this cannot speak about.
#[must_use]
pub fn imported_from(source: &str, name: &str) -> Option<String> {
    qualified(source, name).or_else(|| by_use(source, name))
}

/// The module of a name written out in full at the point it is used.
fn qualified(source: &str, name: &str) -> Option<String> {
    let (before, _rest) = source.split_once(&format!("::{name}"))?;
    let module = before.rsplit("::").next()?;
    module
        .chars()
        .all(|it| it.is_ascii_lowercase() || it.is_ascii_digit() || it == '_')
        .then(|| module.to_owned())
}

fn by_use(source: &str, name: &str) -> Option<String> {
    source
        .lines()
        .filter(|line| line.trim_start().starts_with("use "))
        .find(|line| {
            line.contains(&format!("::{name}"))
                || line.contains(&format!("{{{name}")) && line.contains("::")
                || line.contains(&format!(" {name},"))
                || line.contains(&format!(", {name}"))
        })
        .and_then(|line| {
            let path = line
                .trim_start()
                .strip_prefix("use ")?
                .trim_end_matches(';');
            let head = path.split_once('{').map_or(path, |(head, _rest)| head);
            let head = head.trim().trim_end_matches("::");
            let last = head.rsplit("::").next()?;
            if last == name {
                head.trim_end_matches(name)
                    .trim_end_matches("::")
                    .rsplit("::")
                    .next()
                    .map(str::to_owned)
            } else {
                Some(last.to_owned())
            }
        })
}

/// Every format string that hands a reader a command with an identity in it.
///
/// An identity is a function of the whole file, so the edit a reader makes
/// next — the test that closes the survivor, in the file the survivor is in —
/// re-mints it. A command printed with one in it stops working the moment it
/// is followed, and a configuration record written with one stops naming
/// anything. This finds them by the shape they have: a string that tells
/// somebody what to type, built in the same expression as an identity.
fn handles(file: &str, source: &str) -> Vec<Finding> {
    if file.contains("/tests/") || file.contains("/testkit/") {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_at, line)| {
            HANDED_OUT.iter().any(|said| line.contains(said))
                && PERISHABLE.iter().any(|name| line.contains(name))
        })
        .map(|(at, _line)| Finding {
            kind: Kind::PerishableHandle,
            file: file.to_owned(),
            line: at.saturating_add(1),
        })
        .collect()
}

/// The prefix of a comment that is an instruction to this engine rather than an account of the code beside it.
const ANNOTATION: &str = "rust-mutants:";

/// The prefix of the licence header every file of this repository carries.
const HEADER: &str = "SPDX-";

/// Every comment in `source` that is neither documentation, the licence header, nor an annotation the engine reads.
fn comments(file: &str, source: &str) -> Vec<Finding> {
    let bytes: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    let mut line: usize = 1;
    while let Some(rest) = bytes.get(at..).filter(|rest| !rest.is_empty()) {
        let stepped = |width: usize| newlines(rest.get(..width).unwrap_or(rest));
        if let Some(width) = string_at(rest) {
            line = line.saturating_add(stepped(width));
            at = at.saturating_add(width);
            continue;
        }
        if let Some((width, text, doc)) = comment_at(rest) {
            if !doc && !text.trim_start().starts_with(HEADER) && !names_the_engine(&text) {
                found.push(Finding {
                    kind: Kind::Comment,
                    file: file.to_owned(),
                    line,
                });
            }
            line = line.saturating_add(stepped(width));
            at = at.saturating_add(width);
            continue;
        }
        if bytes.get(at) == Some(&'\n') {
            line = line.saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    found
}

/// Whether the comment is one of the engine's own annotations, which is a thing it reads rather than a thing a person tells another person.
fn names_the_engine(text: &str) -> bool {
    text.trim_start().starts_with(ANNOTATION)
}

/// How many lines a stretch of source covers past the first.
fn newlines(chars: &[char]) -> usize {
    chars.iter().filter(|one| **one == '\n').count()
}

/// The width of the literal starting here, when one does: a string, a raw string of any hash count, a byte or C string of either kind, or a character.
fn string_at(rest: &[char]) -> Option<usize> {
    let mut at = 0;
    while matches!(rest.get(at), Some('b' | 'c')) {
        at = at.saturating_add(1);
    }
    if rest.get(at) == Some(&'r') {
        let mut hashes: usize = 0;
        let mut after = at.saturating_add(1);
        while rest.get(after) == Some(&'#') {
            hashes = hashes.saturating_add(1);
            after = after.saturating_add(1);
        }
        if rest.get(after) == Some(&'"') {
            return Some(raw_string(rest, after.saturating_add(1), hashes));
        }
        return None;
    }
    match rest.get(at) {
        Some('"') => Some(quoted(rest, at.saturating_add(1), '"')),
        Some('\'') if at == 0 => character(rest),
        _ => None,
    }
}

/// The width of a quoted literal whose body starts at `from`, escapes included.
fn quoted(rest: &[char], from: usize, close: char) -> usize {
    let mut at = from;
    while at < rest.len() {
        match rest.get(at) {
            Some('\\') => at = at.saturating_add(2),
            Some(one) if *one == close => return at.saturating_add(1),
            _ => at = at.saturating_add(1),
        }
    }
    rest.len()
}

/// The width of a raw string whose body starts at `from` and closes on `hashes` hashes.
fn raw_string(rest: &[char], from: usize, hashes: usize) -> usize {
    let mut at = from;
    while at < rest.len() {
        if rest.get(at) == Some(&'"')
            && (1..=hashes).all(|step| rest.get(at.saturating_add(step)) == Some(&'#'))
        {
            return at.saturating_add(hashes).saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    rest.len()
}

/// The width of a character literal starting here, or nothing where the quote opens a lifetime.
fn character(rest: &[char]) -> Option<usize> {
    if rest.get(1) == Some(&'\\') {
        let width = quoted(rest, 1, '\'');
        return (width > 1).then_some(width);
    }
    (rest.get(2) == Some(&'\'')).then_some(3)
}

/// The width of the comment starting here, its text, and whether it is documentation.
fn comment_at(rest: &[char]) -> Option<(usize, String, bool)> {
    if rest.first() != Some(&'/') {
        return None;
    }
    match rest.get(1) {
        Some('/') => {
            let doc = matches!(rest.get(2), Some('/' | '!'));
            let end = rest
                .iter()
                .position(|one| *one == '\n')
                .unwrap_or(rest.len());
            let text: String = rest.get(2..end).unwrap_or_default().iter().collect();
            Some((end, text.trim_start_matches(['/', '!']).to_owned(), doc))
        }
        Some('*') => {
            let doc = matches!(rest.get(2), Some('*' | '!'));
            let mut depth = 1usize;
            let mut at = 2;
            while at < rest.len() && depth > 0 {
                if rest.get(at) == Some(&'/') && rest.get(at.saturating_add(1)) == Some(&'*') {
                    depth = depth.saturating_add(1);
                    at = at.saturating_add(2);
                } else if rest.get(at) == Some(&'*') && rest.get(at.saturating_add(1)) == Some(&'/')
                {
                    depth = depth.saturating_sub(1);
                    at = at.saturating_add(2);
                } else {
                    at = at.saturating_add(1);
                }
            }
            let text: String = rest
                .get(2..at.saturating_sub(2))
                .unwrap_or_default()
                .iter()
                .collect();
            Some((at, text, doc))
        }
        _ => None,
    }
}

struct Scan {
    file: String,
    found: Vec<Finding>,
    /// How many loop bodies the walk is inside, which is what makes a removal unbounded.
    looping: usize,
    /// Whether this file is the one that bounds removals, and so may make the call.
    reclaimer: bool,
}

impl Scan {
    /// Walks a loop body, counting it, so a removal inside one is seen as inside one.
    fn within_a_loop(&mut self, walk: impl FnOnce(&mut Self)) {
        self.looping = self.looping.saturating_add(1);
        walk(self);
        self.looping = self.looping.saturating_sub(1);
    }

    fn note(&mut self, kind: Kind, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind,
            file: self.file.clone(),
            line: span.start().line,
        });
    }
}

impl Visit<'_> for Scan {
    fn visit_expr_for_loop(&mut self, loop_: &syn::ExprForLoop) {
        self.within_a_loop(|scan| syn::visit::visit_expr_for_loop(scan, loop_));
    }

    fn visit_expr_while(&mut self, loop_: &syn::ExprWhile) {
        self.within_a_loop(|scan| syn::visit::visit_expr_while(scan, loop_));
    }

    fn visit_expr_loop(&mut self, loop_: &syn::ExprLoop) {
        self.within_a_loop(|scan| syn::visit::visit_expr_loop(scan, loop_));
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if self.looping > 0
            && !self.reclaimer
            && let syn::Expr::Path(path) = call.func.as_ref()
            && path
                .path
                .segments
                .last()
                .is_some_and(|last| last.ident == RAW_REMOVAL)
        {
            let at = path
                .path
                .segments
                .first()
                .map_or_else(proc_macro2::Span::call_site, |one| one.ident.span());
            self.note(Kind::UnboundedRemoval, at);
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_attribute(&mut self, attribute: &syn::Attribute) {
        if attribute.path().is_ident("allow")
            && let Some(segment) = attribute.path().segments.first()
        {
            self.note(Kind::AllowAttribute, segment.ident.span());
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if let Some(segment) = path.path.segments.last()
            && segment.ident == "Box"
            && let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments
            && arguments.args.iter().any(|argument| {
                matches!(
                    argument,
                    syn::GenericArgument::Type(syn::Type::TraitObject(_))
                )
            })
        {
            self.note(Kind::BoxedTraitObject, segment.ident.span());
        }
        syn::visit::visit_type_path(self, path);
    }
}
