// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The critical decisions and what holds each one at every layer, held to the tree: a cell names one definition of the kind its layer is held by, and a hole is one somebody owns.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::visit::Visit;

use super::cfg_conditions::{CfgScope, CfgWorld, item_attributes};

/// One layer a critical decision is held at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Layer {
    /// The decision is made in one place, by a type or a single classifier.
    Types,
    /// The running engine verifies its own output and fails closed.
    SelfCheck,
    /// A test that shares no code with the decision derives the answer on its own.
    Oracle,
    /// A defect planted where the decision is made, and the checks shown to catch it.
    Plant,
    /// rust-mutants run over the module that decides.
    Mutation,
    /// Every state the decision can meet, as rows generated from a closed set.
    States,
}

impl Layer {
    /// The column the registry gives this layer, and the word the gaps ledger names it by.
    #[must_use]
    pub const fn column(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::SelfCheck => "self-check",
            Self::Oracle => "oracle",
            Self::Plant => "plant",
            Self::Mutation => "mutation",
            Self::States => "states",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.column() == word)
    }

    /// What holds this layer.
    const fn holder(self) -> Holder {
        match self {
            Self::Types => Holder::TypeOrFunction,
            Self::SelfCheck => Holder::Reached,
            Self::Oracle | Self::Plant | Self::States => Holder::Test,
            Self::Mutation => Holder::Receipt,
        }
    }
}

/// What kind of item one definition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A struct, an enum, a union, or a type alias.
    Type,
    /// A trait.
    Trait,
    /// A function or a method that is not a test.
    Function,
    /// A `#[test]` function.
    Test,
    /// A test a `proptest!` block declares.
    Property,
    /// A `#[kani::proof]` harness.
    Harness,
    /// A module.
    Module,
    /// A constant or a static.
    Constant,
    /// A `macro_rules!` macro.
    Macro,
}

impl Kind {
    /// The words a refusal names this kind by.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::Type => "a type",
            Self::Trait => "a trait",
            Self::Function => "a function",
            Self::Test => "a test",
            Self::Property => "a property test",
            Self::Harness => "a kani harness",
            Self::Module => "a module",
            Self::Constant => "a constant",
            Self::Macro => "a macro",
        }
    }
}

/// What one layer is held by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    /// A type or a function: the one place the decision is made.
    TypeOrFunction,
    /// A function a production path reaches: the running engine checking what it did.
    Reached,
    /// A test, a property test, or a kani harness.
    Test,
    /// A receipt of a sealed run of rust-mutants over the module that decides, which `receipt::held` holds to the module.
    Receipt,
}

/// What a layer makes of the one definition a cell names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// The definition holds the layer.
    Holds,
    /// The definition is of a kind the layer is not held by.
    OfAnotherKind,
    /// The definition is a function nothing that ships reaches.
    Unreached,
}

impl Holder {
    /// The words a refusal says the layer is held by.
    const fn words(self) -> &'static str {
        match self {
            Self::TypeOrFunction => "a type or a function",
            Self::Reached => "a function a production path reaches",
            Self::Test => "a test, a property test, or a kani harness",
            Self::Receipt => "a receipt of a sealed mutation run under xtask/receipts",
        }
    }

    /// Whether a definition of `kind` can hold the layer at all.
    const fn takes(self, kind: Kind) -> bool {
        match (self, kind) {
            (Self::TypeOrFunction, Kind::Type | Kind::Function)
            | (Self::Reached, Kind::Function)
            | (Self::Test, Kind::Test | Kind::Property | Kind::Harness) => true,
            (
                Self::Receipt,
                Kind::Type
                | Kind::Trait
                | Kind::Function
                | Kind::Test
                | Kind::Property
                | Kind::Harness
                | Kind::Module
                | Kind::Constant
                | Kind::Macro,
            )
            | (
                Self::TypeOrFunction,
                Kind::Trait
                | Kind::Test
                | Kind::Property
                | Kind::Harness
                | Kind::Module
                | Kind::Constant
                | Kind::Macro,
            )
            | (
                Self::Reached,
                Kind::Type
                | Kind::Trait
                | Kind::Test
                | Kind::Property
                | Kind::Harness
                | Kind::Module
                | Kind::Constant
                | Kind::Macro,
            )
            | (
                Self::Test,
                Kind::Type
                | Kind::Trait
                | Kind::Function
                | Kind::Module
                | Kind::Constant
                | Kind::Macro,
            ) => false,
        }
    }

    /// What the layer makes of `definition`, where `reached` is every name production code references.
    fn admits(self, definition: &Definition, reached: &BTreeSet<String>) -> Admission {
        if !self.takes(definition.kind) {
            return Admission::OfAnotherKind;
        }
        match self {
            Self::Reached if !(definition.production && reached.contains(definition.name())) => {
                Admission::Unreached
            }
            Self::Reached | Self::TypeOrFunction | Self::Test | Self::Receipt => Admission::Holds,
        }
    }
}

/// What holds one decision at one layer: the items that do, or nothing yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// The items of the tree that hold it, each by its name or a path ending in it.
    Held(Vec<String>),
    /// Nothing holds it yet, which the gaps ledger has to say somebody owns.
    Open,
}

/// One critical decision and what holds it at every layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The decision's name.
    pub decision: String,
    /// The name the decision had at the base, where this change renames it: its cell reads `new (was old)`.
    pub was: Option<String>,
    /// What holds it, layer by layer.
    pub cells: BTreeMap<Layer, Cell>,
    /// What the oracle cannot see.
    pub blind: String,
}

impl Row {
    /// Every receipt its Mutation cell names.
    pub fn receipts(&self) -> impl Iterator<Item = &str> {
        let names = match self.cells.get(&Layer::Mutation) {
            Some(Cell::Held(names)) => names.as_slice(),
            Some(Cell::Open) | None => &[],
        };
        names.iter().map(String::as_str)
    }
}

/// One hole in the registry, and who owns closing it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap {
    /// The decision.
    pub decision: String,
    /// The layer nothing holds it at.
    pub layer: Layer,
    /// Who closes it.
    pub owner: String,
}

/// One item the tree defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Its module path, from its crate's name to its own, a method under the type it is implemented on.
    pub path: Vec<String>,
    /// What kind of item it is.
    pub kind: Kind,
    /// The file that defines it, repository-relative.
    pub file: String,
    /// The line its name stands on.
    pub line: usize,
    /// Whether a production build of a crate that ships compiles it.
    pub production: bool,
}

impl Definition {
    /// Its own name, the last segment of its path.
    #[must_use]
    pub fn name(&self) -> &str {
        match self.path.last() {
            Some(name) => name,
            None => "",
        }
    }

    /// Its path as a cell writes it qualified.
    #[must_use]
    pub fn qualified(&self) -> String {
        self.path.join("::")
    }

    /// Whether `segments`, a cell's name split at `::`, name it: its path ends with them.
    fn named_by(&self, segments: &[&str]) -> bool {
        self.path.len() >= segments.len()
            && self
                .path
                .iter()
                .rev()
                .zip(segments.iter().rev())
                .all(|(own, named)| own == named)
    }

    /// Where it is and what it is, as a refusal names it.
    fn described(&self) -> String {
        format!(
            "{}, {} ({}:{})",
            self.qualified(),
            self.kind.words(),
            self.file,
            self.line
        )
    }
}

/// Everything the tree defines, and every name production code references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// Every definition of every source.
    pub definitions: Vec<Definition>,
    /// Every name a production path of a crate that ships references, which is what reaches a function.
    pub reached: BTreeSet<String>,
    /// Every receipt a Mutation cell names, by its decision and its name, and whether it holds.
    pub receipts: BTreeMap<(String, String), Receipted>,
}

/// Whether a receipt a Mutation cell names holds its decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipted {
    /// It holds.
    Holds,
    /// It does not, and why.
    Refused {
        /// What the receipt gate said.
        why: String,
    },
}

/// Why the registry and the tree do not hold together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvariantError {
    /// The registry page or the gaps ledger is not in the shape this reads.
    #[error("{source_name}: {detail}")]
    Shape {
        /// The file.
        source_name: String,
        /// What is wrong with it.
        detail: String,
    },
    /// A cell names an item the tree does not define.
    #[error(
        "docs/invariants.md: {decision} is held at {layer} by `{name}`, and the tree defines \
         nothing by that name; a check that is not there holds nothing"
    )]
    Unheld {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// The name.
        name: String,
    },
    /// A cell names more than one definition.
    #[error(
        "docs/invariants.md: {decision} is held at {layer} by `{name}`, which names {} \
         definitions: {}; write it qualified by as much of its module path as says which",
        candidates.len(),
        candidates.join("; ")
    )]
    Ambiguous {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// The name.
        name: String,
        /// Every definition it names, each where it is and what it is.
        candidates: Vec<String>,
    },
    /// A cell names a definition of a kind its layer is not held by.
    #[error(
        "docs/invariants.md: {decision} is held at {layer} by `{name}`, which is {found}, and \
         the {layer} layer is held by {wanted}"
    )]
    Mistyped {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// The name.
        name: String,
        /// The definition it names, where it is and what it is.
        found: String,
        /// What the layer is held by.
        wanted: &'static str,
    },
    /// A self-check cell names a function nothing that ships reaches.
    #[error(
        "docs/invariants.md: {decision} is held at {layer} by `{name}`, which is {found} and \
         which nothing that ships reaches; a check no run makes checks nothing"
    )]
    Unreached {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// The name.
        name: String,
        /// The definition it names, where it is and what it is.
        found: String,
    },
    /// A cell says nothing holds the decision, and the gaps ledger does not say who owns that.
    #[error(
        "docs/invariants.md: nothing holds {decision} at {layer}, and xtask/invariant_gaps.txt \
         names nobody who closes it"
    )]
    Unowned {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
    },
    /// The gaps ledger lists a hole the registry does not have.
    #[error(
        "xtask/invariant_gaps.txt: {decision} {layer} is listed as open for {owner}, and \
         docs/invariants.md says something holds it or has no such decision; take the line out"
    )]
    Stale {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// Who the ledger says owns it.
        owner: String,
    },
    /// A row names an oracle and does not say what that oracle cannot see.
    #[error(
        "docs/invariants.md: {decision} names an oracle and leaves Blind empty; say what the \
         oracle cannot see, so its silence there is not read as coverage"
    )]
    Unblind {
        /// The decision.
        decision: String,
    },
    /// A layer held at the base is open here.
    #[error(
        "docs/invariants.md: {decision} was held at {layer} at the base and is `none` here; a \
         layer that held may not open again"
    )]
    Reopened {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
    },
    /// A Mutation cell names a receipt that does not hold the decision.
    #[error(
        "docs/invariants.md: {decision} is held at mutation by `{name}`, which does not hold: {why}"
    )]
    Unreceipted {
        /// The decision.
        decision: String,
        /// The receipt the cell names.
        name: String,
        /// Why it does not hold.
        why: String,
    },
    /// A decision the base has is gone.
    #[error(
        "docs/invariants.md: {decision} is at the base and not here; keep its row, or rename it \
         by writing the new row's decision as `new-name (was {decision})`, which carries the \
         base row over to it"
    )]
    Vanished {
        /// The decision.
        decision: String,
    },
}

impl crate::error::Coded for InvariantError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Shape { .. }
            | Self::Unheld { .. }
            | Self::Ambiguous { .. }
            | Self::Mistyped { .. }
            | Self::Unreached { .. }
            | Self::Unowned { .. }
            | Self::Stale { .. }
            | Self::Unblind { .. }
            | Self::Reopened { .. }
            | Self::Unreceipted { .. }
            | Self::Vanished { .. } => crate::error::XtCode::InvariantRegistry,
        }
    }
}

/// The columns the registry's table has, in order: the decision, what it promises, a column per layer, and what the oracle cannot see.
const HEADER: [&str; 9] = [
    "Decision",
    "Invariant",
    "Types",
    "Self-check",
    "Oracle",
    "Plant",
    "Mutation",
    "States",
    "Blind",
];

/// The cells of one table line, trimmed, or nothing where the line is not a table line.
fn cells(line: &str) -> Option<Vec<&str>> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}

/// Whether `name` is an item's name or a path ending in one, identifier segments joined by `::`, or a receipt a Mutation cell names.
fn a_name(name: &str) -> bool {
    a_receipt(name)
        || name.split("::").all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Whether `name` is a receipt under `xtask/receipts`: a file name of lowercase words and hyphens ending in `.json`.
fn a_receipt(name: &str) -> bool {
    name.strip_prefix(crate::receipt::DIRECTORY)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|stem| {
            !stem.is_empty()
                && stem
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// The names one layer cell holds, or that it is open.
fn cell(text: &str, decision: &str) -> Result<Cell, InvariantError> {
    if text == "none" {
        return Ok(Cell::Open);
    }
    let mut names = Vec::new();
    for part in text.split(", ") {
        let name = part
            .strip_prefix('`')
            .and_then(|rest| rest.strip_suffix('`'))
            .filter(|name| a_name(name))
            .ok_or_else(|| InvariantError::Shape {
                source_name: "docs/invariants.md".to_owned(),
                detail: format!(
                    "{decision}: a layer cell is `none` or backticked item names, each perhaps \
                     qualified by `::`, or receipts under xtask/receipts, joined by \", \", and \
                     {text:?} is neither"
                ),
            })?;
        names.push(name.to_owned());
    }
    Ok(Cell::Held(names))
}

/// Every row of the registry's table.
///
/// # Errors
/// The page has no table with the registry's columns, a line of it has another number of cells, a layer cell is neither `none` nor names, or a decision is listed twice.
pub fn rows(page: &str) -> Result<Vec<Row>, InvariantError> {
    let shape = |detail: String| InvariantError::Shape {
        source_name: "docs/invariants.md".to_owned(),
        detail,
    };
    let mut lines = page
        .lines()
        .skip_while(|line| cells(line).as_deref() != Some(&HEADER[..]));
    if lines.next().is_none() {
        return Err(shape(format!("no table heads its columns {HEADER:?}")));
    }
    if !lines.next().and_then(cells).is_some_and(|rule| {
        rule.len() == HEADER.len() && rule.iter().all(|dashes| dashes.chars().all(|c| c == '-'))
    }) {
        return Err(shape(
            "the table's header is not followed by its rule".to_owned(),
        ));
    }
    let mut rows: Vec<Row> = Vec::new();
    for line in lines.map_while(cells) {
        let [
            decision,
            _,
            types,
            check,
            oracle,
            plant,
            mutation,
            states,
            blind,
        ] = line.as_slice()
        else {
            return Err(shape(format!(
                "a row has {} cells, and every row has {}",
                line.len(),
                HEADER.len()
            )));
        };
        let (decision, was) = match decision
            .strip_suffix(')')
            .and_then(|head| head.split_once(" (was "))
        {
            Some((name, old)) => (name, Some(old.to_owned())),
            None => (*decision, None),
        };
        if rows.iter().any(|row| row.decision == decision) {
            return Err(shape(format!("{decision} is listed twice")));
        }
        let texts = [types, check, oracle, plant, mutation, states];
        let mut held = BTreeMap::new();
        for (layer, text) in Layer::ALL.into_iter().zip(texts) {
            held.insert(layer, cell(text, decision)?);
        }
        rows.push(Row {
            decision: decision.to_owned(),
            was,
            cells: held,
            blind: (*blind).to_owned(),
        });
    }
    Ok(rows)
}

/// Every hole the gaps ledger lists.
///
/// # Errors
/// A line is not `<decision> <layer> <owner>`, names a layer the registry does not have, or repeats another.
pub fn gaps(ledger: &str) -> Result<Vec<Gap>, InvariantError> {
    let mut gaps: Vec<Gap> = Vec::new();
    for line in ledger
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let shape = |detail: String| InvariantError::Shape {
            source_name: "xtask/invariant_gaps.txt".to_owned(),
            detail,
        };
        let words: Vec<&str> = line.split_whitespace().collect();
        let [decision, layer, owner] = words.as_slice() else {
            return Err(shape(format!(
                "{line:?} is not `<decision> <layer> <owner>`"
            )));
        };
        let layer = Layer::named(layer).ok_or_else(|| {
            shape(format!(
                "{line:?} names a layer the registry has no column for"
            ))
        })?;
        let gap = Gap {
            decision: (*decision).to_owned(),
            layer,
            owner: (*owner).to_owned(),
        };
        if gaps
            .iter()
            .any(|seen| seen.decision == gap.decision && seen.layer == gap.layer)
        {
            return Err(shape(format!("{line:?} is listed twice")));
        }
        gaps.push(gap);
    }
    Ok(gaps)
}

/// The module path of the file at the repository-relative `file`: its crate's name, then every directory and the file's own stem below the crate's `src`, with `tests`, `benches`, `examples` and the like kept as a segment of their own.
#[must_use]
pub fn module_of(file: &str) -> Vec<String> {
    let parts: Vec<&str> = file.split('/').collect();
    let (krate, below) = match parts.as_slice() {
        ["crates", krate, below @ ..] | [krate, below @ ..] => (*krate, below),
        [] => ("", &[][..]),
    };
    let below = match below {
        ["src", rest @ ..] | rest => rest,
    };
    let mut path = vec![krate.replace('-', "_")];
    let Some((last, directories)) = below.split_last() else {
        return path;
    };
    path.extend(
        directories
            .iter()
            .map(|directory| directory.replace('-', "_")),
    );
    let stem = match last.strip_suffix(".rs") {
        Some(stem) => stem,
        None => last,
    };
    if !(matches!(stem, "mod") || (directories.is_empty() && matches!(stem, "lib" | "main"))) {
        path.push(stem.replace('-', "_"));
    }
    path
}

/// Every item `source`, the file at the repository-relative `file`, defines as Rust, under the module path [`module_of`] gives the file.
///
/// `production` says whether the file is compiled into a crate that ships, and a `cfg` a production build cannot meet takes an item out of it.
/// What a comment, a doc or a string says is no definition.
/// A `macro_rules!` body's items are its own, under the macro's module, and so are the tests a `proptest!` block declares.
///
/// # Errors
/// A source this version of `syn` cannot parse.
pub fn definitions(
    file: &str,
    source: &str,
    production: bool,
) -> Result<Vec<Definition>, syn::Error> {
    let parsed = crate::lexed::file(source)?;
    let mut walk = Walk {
        file,
        module: module_of(file),
        production,
        cfg: CfgScope::new(CfgWorld::Production),
        found: Vec::new(),
    };
    walk.within(&parsed.attrs, None, |walk| {
        syn::visit::visit_file(walk, &parsed);
    });
    Ok(walk.found)
}

/// A walk of one file's items, where it stands among them.
struct Walk<'a> {
    file: &'a str,
    module: Vec<String>,
    production: bool,
    cfg: CfgScope,
    found: Vec<Definition>,
}

/// What wrote the tokens a token walk reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Writer {
    /// A `macro_rules!` body, whose items are defined wherever it is invoked.
    Macro,
    /// A `proptest!` block, whose `#[test]` functions are property tests.
    Proptest,
}

/// The attributes written before one item a token walk meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Marked {
    test: bool,
    proof: bool,
}

impl Writer {
    /// The kind of a function these tokens write with `marked` before it.
    const fn function(self, marked: Marked) -> Kind {
        match (self, marked.test, marked.proof) {
            (Self::Proptest, true, _) => Kind::Property,
            (Self::Macro, true, _) => Kind::Test,
            (Self::Macro | Self::Proptest, false, true) => Kind::Harness,
            (Self::Macro | Self::Proptest, false, false) => Kind::Function,
        }
    }
}

impl Walk<'_> {
    /// Walks what `walk` visits with `attributes` in force, under `segment` where one is given.
    fn within(
        &mut self,
        attributes: &[syn::Attribute],
        segment: Option<String>,
        walk: impl FnOnce(&mut Self),
    ) {
        let mark = self.cfg.mark();
        for attribute in attributes {
            self.cfg.push(attribute);
        }
        let depth = self.module.len();
        if let Some(segment) = segment {
            self.module.push(segment);
        }
        walk(self);
        self.module.truncate(depth);
        self.cfg.truncate(mark);
    }

    /// Records `ident` as a definition of `kind` here, compiled into production where `compiled` and the `cfg` in force allow it.
    fn define(&mut self, ident: &syn::Ident, kind: Kind, compiled: bool) {
        let mut path = self.module.clone();
        path.push(ident.unraw().to_string());
        self.found.push(Definition {
            path,
            kind,
            file: self.file.to_owned(),
            line: ident.span().start().line,
            production: compiled && self.production && self.cfg.possible(),
        });
    }

    /// Every item `tokens` write as Rust, which `writer` wrote.
    fn written(&mut self, tokens: TokenStream, writer: Writer) {
        let mut marked = Marked {
            test: false,
            proof: false,
        };
        let mut hash = false;
        let mut introduced: Option<Kind> = None;
        for tree in tokens {
            match tree {
                TokenTree::Punct(punct) => {
                    hash = punct.as_char() == '#';
                    introduced = None;
                    if punct.as_char() == ';' {
                        marked = Marked {
                            test: false,
                            proof: false,
                        };
                    }
                }
                TokenTree::Group(group) => {
                    if hash && group.delimiter() == Delimiter::Bracket {
                        let words: Vec<String> = group
                            .stream()
                            .into_iter()
                            .filter_map(|tree| match tree {
                                TokenTree::Ident(ident) => Some(ident.to_string()),
                                TokenTree::Punct(_)
                                | TokenTree::Group(_)
                                | TokenTree::Literal(_) => None,
                            })
                            .collect();
                        marked.test |= words.last().is_some_and(|last| last == "test");
                        marked.proof |= words == ["kani", "proof"];
                    } else {
                        self.written(group.stream(), writer);
                        if group.delimiter() == Delimiter::Brace {
                            marked = Marked {
                                test: false,
                                proof: false,
                            };
                        }
                    }
                    hash = false;
                    introduced = None;
                }
                TokenTree::Ident(ident) => {
                    hash = false;
                    let word = ident.to_string();
                    if let Some(kind) = introduced.take()
                        && !KEYWORDS.contains(&word.as_str())
                    {
                        let kind = if kind == Kind::Function {
                            writer.function(marked)
                        } else {
                            kind
                        };
                        self.define(&ident, kind, false);
                        marked = Marked {
                            test: false,
                            proof: false,
                        };
                        continue;
                    }
                    introduced = introducer(&word);
                }
                TokenTree::Literal(_) => {
                    hash = false;
                    introduced = None;
                }
            }
        }
    }
}

/// The words Rust reserves, which never name an item a token walk meets.
const KEYWORDS: [&str; 20] = [
    "as", "async", "const", "crate", "enum", "extern", "fn", "impl", "mod", "move", "mut", "pub",
    "ref", "self", "Self", "static", "struct", "super", "trait", "type",
];

/// The kind of item `word` introduces, where it is a keyword that introduces one.
fn introducer(word: &str) -> Option<Kind> {
    match word {
        "fn" => Some(Kind::Function),
        "struct" | "enum" | "type" => Some(Kind::Type),
        "trait" => Some(Kind::Trait),
        "const" | "static" => Some(Kind::Constant),
        "mod" => Some(Kind::Module),
        _ => None,
    }
}

/// The name the type of an `impl` block is known by, where its self type has one.
fn implemented(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.unraw().to_string()),
        syn::Type::Reference(reference) => implemented(&reference.elem),
        syn::Type::Paren(paren) => implemented(&paren.elem),
        syn::Type::Group(group) => implemented(&group.elem),
        _ => None,
    }
}

/// The kind a function with `attributes` is: a test, a kani harness, or a function.
fn function_kind(attributes: &[syn::Attribute]) -> Kind {
    let named = |attribute: &syn::Attribute| -> Vec<String> {
        attribute
            .path()
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect()
    };
    if attributes
        .iter()
        .any(|attribute| named(attribute).last().is_some_and(|last| last == "test"))
    {
        Kind::Test
    } else if attributes
        .iter()
        .any(|attribute| named(attribute) == ["kani", "proof"])
    {
        Kind::Harness
    } else {
        Kind::Function
    }
}

impl<'ast> Visit<'ast> for Walk<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.within(item_attributes(item), None, |walk| {
            syn::visit::visit_item(walk, item);
        });
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.define(&item.ident, Kind::Module, true);
        self.within(&[], Some(item.ident.unraw().to_string()), |walk| {
            syn::visit::visit_item_mod(walk, item);
        });
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.define(&item.sig.ident, function_kind(&item.attrs), true);
        self.within(&[], Some(item.sig.ident.unraw().to_string()), |walk| {
            syn::visit::visit_item_fn(walk, item);
        });
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.define(&item.ident, Kind::Type, true);
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.define(&item.ident, Kind::Type, true);
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        self.define(&item.ident, Kind::Type, true);
        syn::visit::visit_item_union(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.define(&item.ident, Kind::Type, true);
        syn::visit::visit_item_type(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.define(&item.ident, Kind::Constant, true);
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.define(&item.ident, Kind::Constant, true);
        syn::visit::visit_item_static(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.define(&item.ident, Kind::Trait, true);
        self.within(&[], Some(item.ident.unraw().to_string()), |walk| {
            syn::visit::visit_item_trait(walk, item);
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.sig.ident, function_kind(&item.attrs), true);
        });
        self.within(
            &item.attrs,
            Some(item.sig.ident.unraw().to_string()),
            |walk| {
                syn::visit::visit_trait_item_fn(walk, item);
            },
        );
    }

    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.ident, Kind::Constant, true);
            syn::visit::visit_trait_item_const(walk, item);
        });
    }

    fn visit_trait_item_type(&mut self, item: &'ast syn::TraitItemType) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.ident, Kind::Type, true);
            syn::visit::visit_trait_item_type(walk, item);
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.within(&[], implemented(&item.self_ty), |walk| {
            syn::visit::visit_item_impl(walk, item);
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.sig.ident, function_kind(&item.attrs), true);
        });
        self.within(
            &item.attrs,
            Some(item.sig.ident.unraw().to_string()),
            |walk| {
                syn::visit::visit_impl_item_fn(walk, item);
            },
        );
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.ident, Kind::Constant, true);
            syn::visit::visit_impl_item_const(walk, item);
        });
    }

    fn visit_impl_item_type(&mut self, item: &'ast syn::ImplItemType) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.ident, Kind::Type, true);
            syn::visit::visit_impl_item_type(walk, item);
        });
    }

    fn visit_foreign_item_fn(&mut self, item: &'ast syn::ForeignItemFn) {
        self.within(&item.attrs, None, |walk| {
            walk.define(&item.sig.ident, Kind::Function, true);
            syn::visit::visit_foreign_item_fn(walk, item);
        });
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        let named = match item.mac.path.segments.last() {
            Some(segment) => segment.ident.to_string(),
            None => String::new(),
        };
        if item.mac.path.is_ident("macro_rules") {
            if let Some(name) = &item.ident {
                self.define(name, Kind::Macro, true);
            }
            self.written(item.mac.tokens.clone(), Writer::Macro);
        } else if named == "proptest" {
            self.written(item.mac.tokens.clone(), Writer::Proptest);
        }
        syn::visit::visit_item_macro(self, item);
    }
}

/// Every definition the name `name`, split at `::`, names.
fn resolved<'a>(name: &str, definitions: &'a [Definition]) -> Vec<&'a Definition> {
    let segments: Vec<&str> = name.split("::").collect();
    definitions
        .iter()
        .filter(|definition| definition.named_by(&segments))
        .collect()
}

/// Every way the registry, the gaps ledger and the tree disagree, or how many decisions and cells hold.
///
/// Each name of a held cell resolves to exactly one definition of `tree`, of the kind its layer is held by.
///
/// # Errors
/// Each cell name naming no definition, several, or one of another kind than its layer is held by or a self-check nothing that ships reaches; each open cell nobody owns; each listed hole the registry does not have; and each row whose oracle is named with nothing said about what it cannot see.
pub fn check(
    rows: &[Row],
    gaps: &[Gap],
    tree: &Tree,
) -> Result<(usize, usize), Vec<InvariantError>> {
    let mut refused = Vec::new();
    let mut held = 0_usize;
    for row in rows {
        if matches!(row.cells.get(&Layer::Oracle), Some(Cell::Held(_))) && row.blind.is_empty() {
            refused.push(InvariantError::Unblind {
                decision: row.decision.clone(),
            });
        }
        for (layer, cell) in &row.cells {
            match cell {
                Cell::Held(names) => {
                    held = held.saturating_add(1);
                    for name in names {
                        refused.extend(match layer {
                            Layer::Mutation => receipted(row, name, tree),
                            Layer::Types
                            | Layer::SelfCheck
                            | Layer::Oracle
                            | Layer::Plant
                            | Layer::States => resolution(row, *layer, name, tree),
                        });
                    }
                }
                Cell::Open => {
                    if !gaps
                        .iter()
                        .any(|gap| gap.decision == row.decision && gap.layer == *layer)
                    {
                        refused.push(InvariantError::Unowned {
                            decision: row.decision.clone(),
                            layer: layer.column(),
                        });
                    }
                }
            }
        }
    }
    for gap in gaps {
        let open = rows.iter().any(|row| {
            row.decision == gap.decision && row.cells.get(&gap.layer) == Some(&Cell::Open)
        });
        if !open {
            refused.push(InvariantError::Stale {
                decision: gap.decision.clone(),
                layer: gap.layer.column(),
                owner: gap.owner.clone(),
            });
        }
    }
    if refused.is_empty() {
        Ok((rows.len(), held))
    } else {
        Err(refused)
    }
}

/// What is wrong with the receipt `name` holding `row` at mutation, if anything.
fn receipted(row: &Row, name: &str, tree: &Tree) -> Option<InvariantError> {
    let why = match tree.receipts.get(&(row.decision.clone(), name.to_owned())) {
        Some(Receipted::Holds) => return None,
        Some(Receipted::Refused { why }) => why.clone(),
        None => "no receipt of that name was read".to_owned(),
    };
    Some(InvariantError::Unreceipted {
        decision: row.decision.clone(),
        name: name.to_owned(),
        why,
    })
}

/// What is wrong with `name` holding `row` at `layer`, if anything.
fn resolution(row: &Row, layer: Layer, name: &str, tree: &Tree) -> Option<InvariantError> {
    let decision = row.decision.clone();
    let column = layer.column();
    match resolved(name, &tree.definitions).as_slice() {
        [] => Some(InvariantError::Unheld {
            decision,
            layer: column,
            name: name.to_owned(),
        }),
        [one] => {
            let holder = layer.holder();
            match holder.admits(one, &tree.reached) {
                Admission::Holds => None,
                Admission::OfAnotherKind => Some(InvariantError::Mistyped {
                    decision,
                    layer: column,
                    name: name.to_owned(),
                    found: one.described(),
                    wanted: holder.words(),
                }),
                Admission::Unreached => Some(InvariantError::Unreached {
                    decision,
                    layer: column,
                    name: name.to_owned(),
                    found: one.described(),
                }),
            }
        }
        several => Some(InvariantError::Ambiguous {
            decision,
            layer: column,
            name: name.to_owned(),
            candidates: several.iter().map(|one| one.described()).collect(),
        }),
    }
}

/// Every way the registry has fallen back since `base`: a layer held there and open here, and a decision there that is gone, where a row renamed with `new (was old)` carries `old` over.
///
/// A decision the base does not have enters with whatever it owes, since a new decision with owned holes is the registry doing its work.
#[must_use]
pub fn regressions(base: &[Row], head: &[Row]) -> Vec<InvariantError> {
    let mut refused = Vec::new();
    for before in base {
        let now = head.iter().find(|row| {
            row.decision == before.decision || row.was.as_deref() == Some(before.decision.as_str())
        });
        let Some(now) = now else {
            refused.push(InvariantError::Vanished {
                decision: before.decision.clone(),
            });
            continue;
        };
        for (layer, cell) in &before.cells {
            let reopened =
                matches!(cell, Cell::Held(_)) && now.cells.get(layer) == Some(&Cell::Open);
            if reopened {
                refused.push(InvariantError::Reopened {
                    decision: now.decision.clone(),
                    layer: layer.column(),
                });
            }
        }
    }
    refused
}
