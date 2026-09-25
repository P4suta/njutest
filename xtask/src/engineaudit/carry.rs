// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a body is sealed, read again from its source under `docs/engine/carry.md` with this audit's own parser and nothing of the engine's.

use proc_macro2::{LineColumn, TokenStream, TokenTree};
use syn::visit::Visit;

/// The lists a body is held to, as `docs/engine/carry.md`'s fenced blocks name them.
#[derive(Debug, Clone, Copy)]
pub struct Lists<'a> {
    /// `sealable-macros`.
    pub macros: &'a [String],
    /// `standard-roots`.
    pub roots: &'a [String],
    /// `local-roots`.
    pub local: &'a [String],
    /// `sealable-attributes`.
    pub attributes: &'a [String],
    /// `tool-namespaces`.
    pub tools: &'a [String],
}

/// Why a body is not sealed, as the page names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Unsealed {
    /// The catalog's body span is not a function body of the file.
    Unlocated,
    /// A `const fn`, which the compiler can evaluate where nothing enters it.
    Evaluated,
    /// A body of a file a `proc-macro` or `custom-build` unit read, which runs inside the compiler.
    CompileTime,
    /// An `async fn`, or one whose return type holds `impl`, whose body decides a type observed without entering it.
    OpaqueType,
    /// An attribute off the list.
    Attribute,
    /// A macro off the list, or one invoked inside a listed one's arguments.
    Macro,
    /// An item declared inside the body.
    DeclaresItem,
    /// An inline `const` block.
    ConstBlock,
    /// A file of the unit declares or imports a macro under a listed name.
    Shadowed,
    /// A glob import from outside the standard and local roots, or a `#[macro_use] extern crate`, in a file of the unit.
    ForeignGlob,
}

impl Unsealed {
    /// The word the page and the evidence name it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unlocated => "unlocated",
            Self::Evaluated => "evaluated",
            Self::CompileTime => "compile-time",
            Self::OpaqueType => "opaque-type",
            Self::Attribute => "attribute",
            Self::Macro => "macro",
            Self::DeclaresItem => "declares-item",
            Self::ConstBlock => "const-block",
            Self::Shadowed => "shadowed",
            Self::ForeignGlob => "foreign-glob",
        }
    }
}

/// Whether the body whose opening brace is at `start` in `file` is sealed, reading rules 3 to 7 of the page, with `unit` every file the unit read.
///
/// # Errors
/// The first [`Unsealed`] reason, in the page's order.
pub fn sealing(
    file: &syn::File,
    start: LineColumn,
    unit: &[syn::File],
    lists: Lists<'_>,
) -> Result<(), Unsealed> {
    let Some(found) = located(file, start) else {
        return Err(Unsealed::Unlocated);
    };
    if found.constness {
        return Err(Unsealed::Evaluated);
    }
    if !found
        .context
        .iter()
        .chain(&file.attrs)
        .all(|attribute| allowed(attribute, lists))
    {
        return Err(Unsealed::Attribute);
    }
    if found.opaque {
        return Err(Unsealed::OpaqueType);
    }
    let mut first = FirstInBody { lists, found: None };
    first.visit_block(&found.block);
    if let Some(why) = first.found {
        return Err(why);
    }
    for one in unit {
        if let Some(why) = unit_reason(one, lists) {
            return Err(why);
        }
    }
    Ok(())
}

/// A function body the catalog can name, with every attribute around it.
struct Located {
    block: syn::Block,
    constness: bool,
    opaque: bool,
    context: Vec<syn::Attribute>,
}

/// The function whose body's opening brace is at `start`, with the attributes of the item and of every inline module, `impl` or `trait` around it.
fn located(file: &syn::File, start: LineColumn) -> Option<Located> {
    let mut search = Search {
        start,
        context: Vec::new(),
        found: None,
    };
    search.items(&file.items);
    search.found
}

struct Search {
    start: LineColumn,
    context: Vec<syn::Attribute>,
    found: Option<Located>,
}

impl Search {
    fn items(&mut self, items: &[syn::Item]) {
        for item in items {
            if self.found.is_some() {
                return;
            }
            self.item(item);
        }
    }

    fn item(&mut self, item: &syn::Item) {
        match item {
            syn::Item::Fn(function) => {
                self.candidate(&function.block, &function.sig, &function.attrs);
            }
            syn::Item::Mod(module) => {
                if let Some((_, inner)) = &module.content {
                    self.within(&module.attrs, |search| search.items(inner));
                }
            }
            syn::Item::Impl(block) => self.within(&block.attrs, |search| {
                for member in &block.items {
                    if let syn::ImplItem::Fn(method) = member {
                        search.candidate(&method.block, &method.sig, &method.attrs);
                    }
                }
            }),
            syn::Item::Trait(declared) => self.within(&declared.attrs, |search| {
                for member in &declared.items {
                    if let syn::TraitItem::Fn(method) = member
                        && let Some(block) = &method.default
                    {
                        search.candidate(block, &method.sig, &method.attrs);
                    }
                }
            }),
            _ => {}
        }
    }

    fn within(&mut self, attributes: &[syn::Attribute], inside: impl FnOnce(&mut Self)) {
        let depth = self.context.len();
        self.context.extend(attributes.iter().cloned());
        inside(self);
        self.context.truncate(depth);
    }

    fn candidate(
        &mut self,
        block: &syn::Block,
        signature: &syn::Signature,
        attributes: &[syn::Attribute],
    ) {
        if block.brace_token.span.open().start() == self.start {
            let mut context = self.context.clone();
            context.extend(attributes.iter().cloned());
            let mut opaque = HoldsImpl(false);
            opaque.visit_return_type(&signature.output);
            self.found = Some(Located {
                block: block.clone(),
                constness: signature.constness.is_some(),
                opaque: signature.asyncness.is_some() || opaque.0,
                context,
            });
        }
    }
}

/// Whether a type holds `impl` anywhere.
struct HoldsImpl(bool);

impl<'ast> Visit<'ast> for HoldsImpl {
    fn visit_type_impl_trait(&mut self, _: &'ast syn::TypeImplTrait) {
        self.0 = true;
    }
}

/// The first reason inside a body, in source order.
struct FirstInBody<'a> {
    lists: Lists<'a>,
    found: Option<Unsealed>,
}

impl FirstInBody<'_> {
    const fn note(&mut self, why: Unsealed) {
        if self.found.is_none() {
            self.found = Some(why);
        }
    }
}

impl<'ast> Visit<'ast> for FirstInBody<'_> {
    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if !allowed(node, self.lists) {
            self.note(Unsealed::Attribute);
        }
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !listed(&node.path, self.lists) || !invoked_only_listed(&node.tokens, self.lists) {
            self.note(Unsealed::Macro);
        }
    }

    fn visit_item(&mut self, _node: &'ast syn::Item) {
        self.note(Unsealed::DeclaresItem);
    }

    fn visit_expr_const(&mut self, _node: &'ast syn::ExprConst) {
        self.note(Unsealed::ConstBlock);
    }
}

/// Whether `path` is one listed segment, or a standard root and a listed segment.
fn listed(path: &syn::Path, lists: Lists<'_>) -> bool {
    let names: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match names.as_slice() {
        [name] => lists.macros.iter().any(|one| one == name),
        [root, name] => {
            path.leading_colon.is_none()
                && lists.roots.iter().any(|one| one == root)
                && lists.macros.iter().any(|one| one == name)
        }
        _ => false,
    }
}

/// Whether every invocation read from `tokens` as an identifier, `!` and a delimited group, at any depth, names a listed macro.
fn invoked_only_listed(tokens: &TokenStream, lists: Lists<'_>) -> bool {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => {
                if !invoked_only_listed(&group.stream(), lists) {
                    return false;
                }
            }
            TokenTree::Punct(punct) if punct.as_char() == '!' => {
                let delimited = matches!(
                    at.checked_add(1).and_then(|next| trees.get(next)),
                    Some(TokenTree::Group(_))
                );
                let Some(TokenTree::Ident(last)) =
                    at.checked_sub(1).and_then(|before| trees.get(before))
                else {
                    continue;
                };
                let named = last.to_string();
                if delimited && !keyword(&named) && !lists.macros.contains(&named) {
                    return false;
                }
            }
            TokenTree::Punct(_) | TokenTree::Ident(_) | TokenTree::Literal(_) => {}
        }
    }
    true
}

/// Whether `word` is a keyword, which a `!` after it does not make an invocation.
fn keyword(word: &str) -> bool {
    syn::parse_str::<syn::Ident>(word).is_err()
}

/// Whether `path` is an attribute path on the list: one listed segment, or a first segment naming a tool.
fn allowed_path(path: &syn::Path, lists: Lists<'_>) -> bool {
    let first = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string());
    match (path.segments.len(), first) {
        (1, Some(name)) => name != "cfg_attr" && lists.attributes.contains(&name),
        (_, Some(name)) => lists.tools.contains(&name),
        (_, None) => false,
    }
}

/// Whether `attribute` is on the list, a `cfg_attr` being on it when every attribute it would apply is.
fn allowed(attribute: &syn::Attribute, lists: Lists<'_>) -> bool {
    let path = attribute.path();
    if !path.is_ident("cfg_attr") {
        return allowed_path(path, lists);
    }
    let Ok(list) = attribute.meta.require_list() else {
        return false;
    };
    let Ok(parts) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        return false;
    };
    parts
        .iter()
        .skip(1)
        .all(|meta| allowed_path(meta.path(), lists))
}

/// The first rule-7 reason in one file of the unit.
fn unit_reason(file: &syn::File, lists: Lists<'_>) -> Option<Unsealed> {
    let mut found = UnitScan { lists, found: None };
    found.visit_file(file);
    found.found
}

struct UnitScan<'a> {
    lists: Lists<'a>,
    found: Option<Unsealed>,
}

impl<'ast> Visit<'ast> for UnitScan<'_> {
    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if self.found.is_none()
            && node.mac.path.is_ident("macro_rules")
            && node
                .ident
                .as_ref()
                .is_some_and(|name| self.lists.macros.iter().any(|listed| name == listed))
        {
            self.found = Some(Unsealed::Shadowed);
        }
        syn::visit::visit_item_macro(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if self.found.is_none() {
            self.found = used(&node.tree, None, self.lists);
        }
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        let standard = self.lists.roots.iter().any(|root| node.ident == root);
        if self.found.is_none()
            && !standard
            && node
                .attrs
                .iter()
                .any(|attribute| attribute.path().is_ident("macro_use"))
        {
            self.found = Some(Unsealed::ForeignGlob);
        }
    }
}

/// What a `use` tree under the root `root` makes visible that unseals: a listed name from outside the standard roots, or a glob from outside the standard and local roots.
fn used(tree: &syn::UseTree, root: Option<&syn::Ident>, lists: Lists<'_>) -> Option<Unsealed> {
    let standard = |ident: &syn::Ident| lists.roots.iter().any(|one| ident == one);
    let local = |ident: &syn::Ident| lists.local.iter().any(|one| ident == one);
    let listed_name = |ident: &syn::Ident| lists.macros.iter().any(|one| ident == one);
    let foreign = root.is_some_and(|root| !standard(root));
    match tree {
        syn::UseTree::Path(path) => match root {
            Some(root) => used(&path.tree, Some(root), lists),
            None => used(&path.tree, Some(&path.ident), lists),
        },
        syn::UseTree::Name(name) => {
            (foreign && listed_name(&name.ident)).then_some(Unsealed::Shadowed)
        }
        syn::UseTree::Rename(rename) => {
            (foreign && listed_name(&rename.rename)).then_some(Unsealed::Shadowed)
        }
        syn::UseTree::Glob(_) => root
            .is_some_and(|root| !standard(root) && !local(root))
            .then_some(Unsealed::ForeignGlob),
        syn::UseTree::Group(group) => group.items.iter().find_map(|one| used(one, root, lists)),
    }
}

/// The lists `docs/engine/carry.md` states, one per fenced block, in the order [`Lists`] names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLists {
    /// `sealable-macros`.
    pub macros: Vec<String>,
    /// `standard-roots`.
    pub roots: Vec<String>,
    /// `local-roots`.
    pub local: Vec<String>,
    /// `sealable-attributes`.
    pub attributes: Vec<String>,
    /// `tool-namespaces`.
    pub tools: Vec<String>,
}

/// Why the page cannot give its lists.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PageError {
    /// A fenced block the page must hold is absent or unclosed.
    #[error("docs/engine/carry.md holds no closed `{name}` block")]
    MissingBlock {
        /// The block's info string.
        name: &'static str,
    },
}

impl crate::error::Coded for PageError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::MissingBlock { .. } => crate::error::XtCode::CarryPage,
        }
    }
}

impl PageLists {
    /// The lists of the page `text`.
    ///
    /// # Errors
    /// A fenced block the page does not hold, or does not close.
    pub fn read(text: &str) -> Result<Self, PageError> {
        let block = |name: &'static str| -> Result<Vec<String>, PageError> {
            let missing = PageError::MissingBlock { name };
            let (_, after) = text
                .split_once(&format!("```{name}\n"))
                .ok_or_else(|| missing.clone())?;
            let (body, _) = after.split_once("```").ok_or(missing)?;
            Ok(body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect())
        };
        Ok(Self {
            macros: block("sealable-macros")?,
            roots: block("standard-roots")?,
            local: block("local-roots")?,
            attributes: block("sealable-attributes")?,
            tools: block("tool-namespaces")?,
        })
    }

    /// The lists as [`sealing`] reads them.
    #[must_use]
    pub fn lists(&self) -> Lists<'_> {
        Lists {
            macros: &self.macros,
            roots: &self.roots,
            local: &self.local,
            attributes: &self.attributes,
            tools: &self.tools,
        }
    }
}

/// Where the byte at `offset` of `text` sits as the parser counts lines and columns, or `None` past its end or inside a character.
#[must_use]
pub fn line_column(text: &str, offset: usize) -> Option<LineColumn> {
    let before = text.get(..offset)?;
    let line = before.matches('\n').count().checked_add(1)?;
    let last = match before.rsplit_once('\n') {
        Some((_, last)) => last,
        None => before,
    };
    let column = last.chars().count();
    Some(LineColumn { line, column })
}

/// The page this audit reads its lists from, as this commit holds it.
const PAGE: &str = include_str!("../../../docs/engine/carry.md");

/// The skeletons document a run kept, as this audit reads it: closed, so a field the engine stopped writing is refused rather than read as absent.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Skeletons {
    document_type: String,
    schema_version: u64,
    items: Vec<ItemClaim>,
    units: Vec<Unit>,
}

/// What the run claims of one cataloged item's body.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemClaim {
    index: u64,
    item: NamedItem,
    name: String,
    body_digest: String,
    sealed: bool,
    #[serde(rename = "unsealed", deserialize_with = "super::wire::required_option")]
    _unsealed: Option<serde_json::Value>,
}

/// One item as the carry evidence names it: its package, its file, and its place among that file's cataloged items.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedItem {
    package: String,
    path: String,
    ordinal: u64,
}

impl std::fmt::Display for NamedItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}#{}", self.package, self.path, self.ordinal)
    }
}

/// One compiled unit's skeleton and every entry it folds.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Unit {
    package: String,
    target: String,
    kind: String,
    test: bool,
    skeleton: String,
    entries: std::collections::BTreeMap<String, String>,
}

/// Every carried record a run believed, as this audit reads it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Believed {
    document_type: String,
    schema_version: u64,
    records: Vec<BelievedRecord>,
}

/// One carried record a run believed, with the plan it was held to.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BelievedRecord {
    mutant: String,
    record: Carried,
    plan: Vec<Planned>,
}

/// An answer from an earlier tree and every execution it rests on.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Carried {
    #[serde(rename = "schema")]
    _schema: String,
    locus: Locus,
    #[serde(rename = "keyed")]
    _keyed: serde_json::Value,
    outcome: String,
    #[serde(rename = "target")]
    _target: String,
    #[serde(
        rename = "tests_run",
        deserialize_with = "super::wire::required_option"
    )]
    _tests_run: Option<u64>,
    #[serde(rename = "failed_tests")]
    _failed_tests: Vec<String>,
    run_id: String,
    executions: Vec<Execution>,
}

/// Where a mutation sits, by nothing outside the item it edits.
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Locus {
    item: NamedItem,
    body_digest: String,
    start: u64,
    end: u64,
    replacement: String,
    rule: String,
}

/// One execution a carried answer rests on.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    target: String,
    #[serde(deserialize_with = "super::wire::required_option")]
    filter: Option<Vec<String>>,
    skeleton: String,
    entered: Vec<Entered>,
    completeness: String,
    detected: bool,
}

/// One item an execution entered, with the digest its body had.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entered {
    item: NamedItem,
    body_digest: String,
}

/// One execution the run's route would have made.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Planned {
    target: String,
    #[serde(deserialize_with = "super::wire::required_option")]
    filter: Option<Vec<String>>,
}

/// The tests a filter names, as a set.
fn tests_of(filter: &[String]) -> std::collections::BTreeSet<&str> {
    filter.iter().map(String::as_str).collect()
}

/// Every item's file and body span, by the item index the guards' record names it by.
type Spans = std::collections::BTreeMap<u64, (String, std::ops::Range<u64>)>;

/// Why the guards' record gives no body span to read an item by.
#[derive(Debug, Clone, Copy)]
enum Unspanned {
    /// The record lists no items.
    NoItems,
    /// An item of it lacks an index, a file or a body span.
    ItemWithoutSpan,
}

impl Unspanned {
    /// The refusal as a sentence.
    const fn said(self) -> &'static str {
        match self {
            Self::NoItems => "the guards' record names no items",
            Self::ItemWithoutSpan => {
                "an item of the guards' record names no index, file or body span"
            }
        }
    }
}

/// The body span of every item the guards' record names, by item index, or why that record cannot say.
fn spans(touched: &serde_json::Value) -> Result<Spans, Unspanned> {
    let Some(items) = touched.get("items").and_then(serde_json::Value::as_array) else {
        return Err(Unspanned::NoItems);
    };
    let mut spans = std::collections::BTreeMap::new();
    for item in items {
        let span = (|| {
            let body = item.get("body")?;
            Some((
                item.get("index")?.as_u64()?,
                item.get("path")?.as_str()?.to_owned(),
                body.get("start")?.as_u64()?..body.get("end")?.as_u64()?,
            ))
        })();
        let Some((index, path, body)) = span else {
            return Err(Unspanned::ItemWithoutSpan);
        };
        spans.insert(index, (path, body));
    }
    Ok(spans)
}

/// One cataloged item, as the guards' record and the carry evidence name it.
struct Cataloged {
    index: u64,
    path: String,
    claimed: (String, u64),
    name: String,
    body: std::ops::Range<usize>,
    digest: String,
    sealed: bool,
}

/// Every item the carry evidence names, joined to the body span the guards' record gives it; an item it cannot be joined to is said.
fn cataloged(skeletons: &Skeletons, spans: &Spans, notes: &mut super::Notes<'_>) -> Vec<Cataloged> {
    let mut found = Vec::new();
    for item in &skeletons.items {
        let Some((path, body)) = spans.get(&item.index) else {
            notes.violated(
                &format!("{}#{}", item.item.path, item.index),
                format!(
                    "the carry evidence names item {} ({}), which the guards' record does not",
                    item.index, item.name
                ),
            );
            continue;
        };
        let (Ok(start), Ok(end)) = (usize::try_from(body.start), usize::try_from(body.end)) else {
            notes.unaudited(
                &format!("{path}#{}", item.index),
                "the body span does not fit this platform's offsets".to_owned(),
            );
            continue;
        };
        found.push(Cataloged {
            index: item.index,
            path: path.clone(),
            claimed: (item.item.path.clone(), item.item.ordinal),
            name: item.name.clone(),
            body: start..end,
            digest: item.body_digest.clone(),
            sealed: item.sealed,
        });
    }
    found
}

/// Every item's name held to the guards' record: its file, and its place among that file's items in catalog order.
fn refs(items: &[Cataloged], notes: &mut super::Notes<'_>) {
    let mut files: std::collections::BTreeMap<&str, Vec<&Cataloged>> =
        std::collections::BTreeMap::new();
    for item in items {
        files.entry(item.path.as_str()).or_default().push(item);
    }
    for file in files.values_mut() {
        file.sort_by_key(|item| item.index);
    }
    for (ordinal, item) in files.values().flat_map(|file| file.iter().enumerate()) {
        if item.claimed.0 != item.path || usize::try_from(item.claimed.1) != Ok(ordinal) {
            notes.violated(
                &format!("{}#{}", item.path, item.index),
                format!(
                    "{} is named {}#{}, and the guards' record makes it {}#{ordinal}",
                    item.name, item.claimed.0, item.claimed.1, item.path
                ),
            );
        }
    }
}

/// The lowercase hex SHA-256 of `bytes`, as every digest the carry evidence keeps is spelled.
#[must_use]
pub fn digest_of(bytes: &[u8]) -> String {
    sha256(bytes)
}

/// The lowercase hex SHA-256 of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// One document the carry layer reads, decoded into the shape this audit expects, or the violation that it is not that shape.
fn decoded<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    (named, document_type): (&str, &str),
    notes: &mut super::Notes<'_>,
) -> Option<T> {
    match serde_json::from_value::<T>(value.clone()) {
        Ok(document) => Some(document),
        Err(error) => {
            notes.violated(
                named,
                format!("{named} is not the {document_type} document this audit reads: {error}"),
            );
            None
        }
    }
}

/// The skeletons document as the shape this audit reads, and the guards' body spans where the run kept them; nothing where either says it is another document.
fn documents(
    skeletons: &serde_json::Value,
    touched: Option<&serde_json::Value>,
    notes: &mut super::Notes<'_>,
) -> Option<(Skeletons, Option<Spans>)> {
    let skeletons = decoded::<Skeletons>(
        skeletons,
        ("skeletons-v1.json", "rust-mutants/skeletons"),
        notes,
    )?;
    if skeletons.document_type != "rust-mutants/skeletons" || skeletons.schema_version != 1 {
        notes.violated(
            "skeletons-v1.json",
            format!(
                "the document says it is {} version {}, not rust-mutants/skeletons version 1",
                skeletons.document_type, skeletons.schema_version
            ),
        );
        return None;
    }
    match touched.map(spans) {
        None => Some((skeletons, None)),
        Some(Ok(spans)) => Some((skeletons, Some(spans))),
        Some(Err(why)) => {
            notes.violated("touched-v1.json", why.said().to_owned());
            None
        }
    }
}

/// The carry evidence against the tree the run measured: every body digest, every body the run calls sealed, every `$root` entry's placeholder rendering, every skeleton's fold, and every carried answer the run believed.
pub(super) fn layer(
    report: &super::Report,
    evidence: &super::CheckedEvidence<'_>,
    audit: &mut super::Audit,
) -> super::Decided {
    let mut notes = super::Notes::on(audit, super::Layer::Carry);
    let Some(skeletons) = evidence.skeletons.as_ref() else {
        return notes.absent("the run kept no skeletons, so it carried no answer to read again");
    };
    let Some((skeletons, spans)) = documents(skeletons, evidence.touched.as_ref(), &mut notes)
    else {
        return notes.looked();
    };
    if let (Some(carried), Some(touched), Some(spans)) = (
        evidence.carried.as_ref(),
        evidence.touched.as_ref(),
        spans.as_ref(),
    ) {
        let standings = evidence
            .recorded
            .as_ref()
            .map(|recorded| crate::drift::standings(&crate::drift::of_events(&recorded.events)));
        if let Some(carried) = decoded::<Believed>(
            carried,
            ("carried-v1.json", "rust-mutants/carried"),
            &mut notes,
        ) {
            believed(
                report,
                (
                    &carried,
                    &Held::of(&skeletons, touched, spans),
                    standings.as_ref(),
                ),
                &mut notes,
            );
        }
    }
    let Some(root) = evidence.root else {
        notes.unaudited(
            "root",
            "no --root names the tree the run measured, so its carry evidence is not read again"
                .to_owned(),
        );
        return notes.looked();
    };
    let Some(spans) = spans else {
        notes.unaudited(
            "touched",
            "the run kept no record of its items' body spans, so no body can be read again"
                .to_owned(),
        );
        return notes.looked();
    };
    let page = match PageLists::read(PAGE) {
        Ok(page) => page,
        Err(refusal) => {
            notes.unaudited("page", format!("{refusal}, so no list is read"));
            return notes.looked();
        }
    };
    let measured: std::collections::BTreeMap<&str, &str> = report
        .mutants
        .iter()
        .map(|row| (row.path.as_str(), row.source_digest.as_str()))
        .collect();
    let items = cataloged(&skeletons, &spans, &mut notes);
    let mut read = Tree::new(root, &measured);
    for item in &items {
        bodies(item, &mut read, (page.lists(), &skeletons), &mut notes);
    }
    refs(&items, &mut notes);
    skeleton_folds(&skeletons, &items, &mut read, &mut notes);
    notes.looked()
}

/// The files of the measured tree, each read once and proved to be the file the run measured where the report can say.
struct Tree<'a> {
    root: &'a std::path::Path,
    measured: &'a std::collections::BTreeMap<&'a str, &'a str>,
    files: std::collections::BTreeMap<String, Option<(String, bool)>>,
}

impl<'a> Tree<'a> {
    const fn new(
        root: &'a std::path::Path,
        measured: &'a std::collections::BTreeMap<&'a str, &'a str>,
    ) -> Self {
        Self {
            root,
            measured,
            files: std::collections::BTreeMap::new(),
        }
    }

    /// The text of `path`, and whether the report's own digest of it proves it is the file measured; `None` where it cannot be read or a digest says it is another file.
    fn text(&mut self, path: &str) -> Option<(String, bool)> {
        if let Some(known) = self.files.get(path) {
            return known.clone();
        }
        let read = match std::fs::read_to_string(self.root.join(path)) {
            Err(_unreadable) => None,
            Ok(text) => match self.measured.get(path) {
                Some(digest) if sha256(text.as_bytes()) != *digest => None,
                Some(_) => Some((text, true)),
                None => Some((text, false)),
            },
        };
        self.files.insert(path.to_owned(), read.clone());
        read
    }
}

/// One item's body digest and sealing, read again.
fn bodies(
    item: &Cataloged,
    tree: &mut Tree<'_>,
    (lists, skeletons): (Lists<'_>, &Skeletons),
    notes: &mut super::Notes<'_>,
) {
    let subject = format!("{}#{}", item.path, item.index);
    let Some((text, proven)) = tree.text(&item.path) else {
        notes.unaudited(
            &subject,
            "the file cannot be read from --root, or is not the one the run measured".to_owned(),
        );
        return;
    };
    let Some(body) = text.as_bytes().get(item.body.clone()) else {
        notes.violated(
            &subject,
            "the item's body span lies outside the file the run measured".to_owned(),
        );
        return;
    };
    if sha256(body) != item.digest {
        let detail = format!(
            "{} keeps a body digest its body's bytes do not hash to, so an edit inside it would \
             not be told from none",
            item.name
        );
        if proven {
            notes.violated(&subject, detail);
        } else {
            notes.unaudited(&subject, detail);
        }
        return;
    }
    if !item.sealed {
        return;
    }
    if read_by_the_compiler(&item.path, skeletons) {
        notes.violated(
            &subject,
            format!(
                "the run calls {} sealed and the page says it is not: {}",
                item.name,
                Unsealed::CompileTime.name()
            ),
        );
        return;
    }
    let (Ok(file), Some(start)) = (syn::parse_file(&text), line_column(&text, item.body.start))
    else {
        notes.violated(
            &subject,
            format!(
                "{} is called sealed in a file the page's parser cannot read",
                item.name
            ),
        );
        return;
    };
    let Some(unit) = unit_files(&item.path, skeletons, tree) else {
        notes.unaudited(
            &subject,
            format!(
                "{} is called sealed, and a Rust file its unit read cannot be read from --root, \
                 so whether that file shadows a listed macro is not known",
                item.name
            ),
        );
        return;
    };
    if let Err(why) = sealing(&file, start, &unit, lists) {
        notes.violated(
            &subject,
            format!(
                "the run calls {} sealed and the page says it is not: {}",
                item.name,
                why.name()
            ),
        );
    }
}

/// Whether a unit that runs inside the compiler, a `proc-macro` or a `custom-build` one, read `path`.
fn read_by_the_compiler(path: &str, skeletons: &Skeletons) -> bool {
    let named = format!("$root/{path}");
    skeletons
        .units
        .iter()
        .filter(|unit| unit.entries.contains_key(&named))
        .any(|unit| {
            unit.kind
                .split(',')
                .any(|one| one == "proc-macro" || one == "custom-build")
        })
}

/// Every file read by any unit that read `path` that parses as a whole Rust file, whatever its extension, or `None` where one of them cannot be read: a `$target` entry, or a `$root` one --root does not hold.
fn unit_files(path: &str, skeletons: &Skeletons, tree: &mut Tree<'_>) -> Option<Vec<syn::File>> {
    let named = format!("$root/{path}");
    let mut files = Vec::new();
    for unit in skeletons
        .units
        .iter()
        .filter(|unit| unit.entries.contains_key(&named))
    {
        for entry in unit.entries.keys() {
            if !entry.starts_with("$root/") && !entry.starts_with("$target/") {
                continue;
            }
            let file = entry.strip_prefix("$root/")?;
            let (text, _) = tree.text(file)?;
            let Ok(parsed) = syn::parse_file(&text) else {
                continue;
            };
            files.push(parsed);
        }
    }
    Some(files)
}

/// Every unit's skeleton against the fold of its entries, and every `$root` entry against this audit's own placeholder rendering of the file.
fn skeleton_folds(
    skeletons: &Skeletons,
    items: &[Cataloged],
    tree: &mut Tree<'_>,
    notes: &mut super::Notes<'_>,
) {
    for unit in &skeletons.units {
        let name = format!("{}/{}", unit.package, unit.target);
        let mut folded = String::new();
        for (entry, digest) in &unit.entries {
            folded.push_str(entry);
            folded.push('\0');
            folded.push_str(digest);
            folded.push('\n');
        }
        if unit.skeleton != sha256(folded.as_bytes()) {
            notes.violated(
                &name,
                "the unit's skeleton is not the fold of the entries it keeps".to_owned(),
            );
        }
        for (entry, digest) in &unit.entries {
            let Some(path) = entry.strip_prefix("$root/") else {
                continue;
            };
            let Some((text, _)) = tree.text(path) else {
                continue;
            };
            let Some(rendered) = rendering(entry, path, &text, items) else {
                notes.violated(
                    &name,
                    format!(
                        "a sealed body of {entry} lies outside the file, so it cannot be set aside"
                    ),
                );
                continue;
            };
            if *digest != sha256(rendered.as_bytes()) {
                notes.violated(
                    &name,
                    format!(
                        "{entry} is kept as a digest that the file with its sealed bodies set \
                         aside does not hash to"
                    ),
                );
            }
        }
    }
}

/// `text` with every sealed body of the file at `path` replaced by the placeholder naming it, as the page defines it, or nothing where a body's span is not in the text.
fn rendering(entry: &str, path: &str, text: &str, items: &[Cataloged]) -> Option<String> {
    let mut ordered: Vec<&Cataloged> = items.iter().filter(|item| item.path == path).collect();
    ordered.sort_by_key(|item| item.index);
    let mut rendered = String::new();
    let mut at = 0_usize;
    for (ordinal, item) in ordered.iter().enumerate() {
        if !item.sealed {
            continue;
        }
        rendered.push_str(text.get(at..item.body.start)?);
        let body = text.get(item.body.clone())?;
        let last = match body.rsplit_once('\n') {
            Some((_, last)) => last,
            None => body,
        };
        rendered.push_str("{sealed:");
        rendered.push_str(entry);
        rendered.push('#');
        rendered.push_str(&ordinal.to_string());
        rendered.push('/');
        rendered.push_str(&body.matches('\n').count().to_string());
        rendered.push(':');
        rendered.push_str(&last.len().to_string());
        rendered.push(':');
        rendered.push_str(&last.chars().count().to_string());
        rendered.push('}');
        at = item.body.end;
    }
    rendered.push_str(text.get(at..)?);
    Some(rendered)
}

/// The evidence a believed record is held to, read once.
struct Held<'a> {
    tree: String,
    bodies: std::collections::BTreeMap<NamedItem, (String, bool)>,
    by_index: std::collections::BTreeMap<u64, (NamedItem, String)>,
    spans: &'a Spans,
    touched: &'a serde_json::Value,
}

impl<'a> Held<'a> {
    fn of(skeletons: &Skeletons, touched: &'a serde_json::Value, spans: &'a Spans) -> Self {
        let mut units: Vec<String> = skeletons
            .units
            .iter()
            .map(|unit| {
                format!(
                    "{}\0{}\0{}\0{}\0{}\n",
                    unit.package, unit.target, unit.kind, unit.test, unit.skeleton
                )
            })
            .collect();
        units.sort();
        let mut bodies = std::collections::BTreeMap::new();
        let mut by_index = std::collections::BTreeMap::new();
        for item in &skeletons.items {
            bodies.insert(item.item.clone(), (item.body_digest.clone(), item.sealed));
            by_index.insert(item.index, (item.item.clone(), item.body_digest.clone()));
        }
        Self {
            tree: sha256(units.concat().as_bytes()),
            bodies,
            by_index,
            spans,
            touched,
        }
    }
}

/// Every carried answer the run believed, held to this run's own evidence: its report row, its locus, the tree's skeleton, what each execution entered, and the plan it was held to (ADR 0041).
fn believed(
    report: &super::Report,
    (carried, held, standings): (
        &Believed,
        &Held<'_>,
        Option<&std::collections::BTreeMap<String, crate::drift::Standing>>,
    ),
    notes: &mut super::Notes<'_>,
) {
    if carried.document_type != "rust-mutants/carried" || carried.schema_version != 1 {
        notes.violated(
            "carried-v1.json",
            format!(
                "the document says it is {} version {}, not rust-mutants/carried version 1",
                carried.document_type, carried.schema_version
            ),
        );
        return;
    }
    if standings.is_none() && !carried.records.is_empty() {
        notes.unaudited(
            "reach",
            "no recording was given, so whether each target a carried answer rests on held its \
             reach under a control of this tree cannot be re-derived"
                .to_owned(),
        );
    }
    for entry in &carried.records {
        let Some(row) = report.mutants.iter().find(|row| row.id == entry.mutant) else {
            notes.violated(
                &entry.mutant,
                "the run believed a carried record about a mutant its report does not hold"
                    .to_owned(),
            );
            continue;
        };
        let subject = row.display_id.as_str();
        let record = &entry.record;
        if record.outcome != row.outcome.as_str()
            || Some(record.run_id.as_str()) != row.source_run_id.as_deref()
        {
            notes.violated(
                subject,
                format!(
                    "the report says {} from {:?}, and the record it carried says {} from {}",
                    row.outcome.as_str(),
                    row.source_run_id,
                    record.outcome,
                    record.run_id
                ),
            );
        }
        if let Some(why) = locus_differs(row, &record.locus, held) {
            notes.violated(
                subject,
                format!("the record's locus is not the mutation's: {why}"),
            );
        }
        if let Some(why) = premise_fails(record, &entry.plan, held) {
            notes.violated(
                subject,
                format!("the run carried it though a premise of ADR 0041 fails: {why}"),
            );
        }
        if let Some(standings) = standings {
            reach_held(
                subject,
                resting_targets(record, &entry.plan),
                standings,
                notes,
            );
        }
        planned_reach(subject, (row.index, &entry.plan), held, notes);
    }
}

/// Whether every target the plan runs is one the guards' record says reaches the mutation at `index`, narrowed to the tests the plan names.
fn planned_reach(
    subject: &str,
    (index, plan): (u64, &[Planned]),
    held: &Held<'_>,
    notes: &mut super::Notes<'_>,
) {
    let reaching = super::evidence::reaching_targets(held.touched, index);
    for planned in plan {
        let target = planned.target.as_str();
        let Some(narrowed) = reaching.get(target) else {
            notes.violated(
                subject,
                format!(
                    "the plan runs {target}, which the guards' record says reaches nothing of it"
                ),
            );
            continue;
        };
        let filter: Option<std::collections::BTreeSet<String>> = planned
            .filter
            .as_ref()
            .map(|tests| tests.iter().cloned().collect());
        if filter.is_some() && filter.as_ref() != narrowed.as_ref() {
            notes.violated(
                subject,
                format!(
                    "the plan narrows {target} to {filter:?}, and the guards' record narrows it \
                     to {narrowed:?}"
                ),
            );
        }
    }
}

/// Whether every target a carried answer rests on held its reach, re-derived from the control records the run kept (P7).
fn reach_held(
    subject: &str,
    targets: Vec<&str>,
    standings: &std::collections::BTreeMap<String, crate::drift::Standing>,
    notes: &mut super::Notes<'_>,
) {
    for target in targets {
        let standing = match standings.get(target) {
            Some(crate::drift::Standing::Held) => continue,
            Some(other) => other.name(),
            None => "without a baseline to compare a control with",
        };
        notes.violated(
            subject,
            format!(
                "the run carried it though a premise of ADR 0041 fails: reach-moved: the run's own \
                 records make {target} {standing}"
            ),
        );
    }
}

/// Every target a carried answer rests on: the killer's for a kill, every planned one for a survival.
fn resting_targets<'r>(record: &'r Carried, plan: &'r [Planned]) -> Vec<&'r str> {
    if record.outcome == "killed" {
        return record
            .executions
            .iter()
            .rev()
            .find(|one| one.detected)
            .map(|killer| killer.target.as_str())
            .into_iter()
            .collect();
    }
    plan.iter().map(|one| one.target.as_str()).collect()
}

/// Why a record's locus is not the mutation at `row`: another item, another body, another place in it, another edit, or another rule.
fn locus_differs(row: &super::Row, locus: &Locus, held: &Held<'_>) -> Option<String> {
    let Some((index, (_, body))) = held
        .spans
        .iter()
        .filter(|(_, (path, body))| {
            *path == row.path && body.start <= row.start_byte && row.end_byte <= body.end
        })
        .max_by_key(|(_, (_, body))| body.start)
    else {
        return Some("the mutation is inside no item body, which has no locus".to_owned());
    };
    let Some((item, digest)) = held.by_index.get(index) else {
        return Some(format!("item {index} has no carry evidence"));
    };
    let (Some(start), Some(end)) = (
        row.start_byte.checked_sub(body.start),
        row.end_byte.checked_sub(body.start),
    ) else {
        return Some("the mutation starts before the body it is inside".to_owned());
    };
    let expected = Locus {
        item: item.clone(),
        body_digest: digest.clone(),
        start,
        end,
        replacement: row.replacement.clone(),
        rule: format!("{}@{}", row.rule, row.rule_version),
    };
    (*locus != expected)
        .then(|| format!("it says {locus:?}, and the evidence makes it {expected:?}"))
}

/// The first premise of ADR 0041 a believed record fails against this run's evidence, in the words the trace uses.
fn premise_fails(record: &Carried, plan: &[Planned], held: &Held<'_>) -> Option<String> {
    let resting: Vec<(&Execution, &[&str])> = match record.outcome.as_str() {
        "killed" => {
            let Some(killer) = record.executions.iter().rev().find(|one| one.detected) else {
                return Some("entry-incomplete: a kill no execution detected".to_owned());
            };
            let Some(planned) = plan.iter().find(|one| one.target == killer.target) else {
                return Some("filter-differs: the killer's target is not in the plan".to_owned());
            };
            if planned.filter.as_deref().map(tests_of) != killer.filter.as_deref().map(tests_of) {
                return Some("filter-differs: the killer named other tests".to_owned());
            }
            vec![(killer, &["whole", "up-to-first-failure"])]
        }
        "survived" => {
            let mut resting = Vec::new();
            for planned in plan {
                let mut ran = record
                    .executions
                    .iter()
                    .filter(|one| one.target == planned.target)
                    .peekable();
                if ran.peek().is_none() {
                    return Some(format!(
                        "route-grew: nothing recorded ran {}",
                        planned.target
                    ));
                }
                let Some(execution) = ran.find(|one| {
                    one.filter.as_deref().map(tests_of) == planned.filter.as_deref().map(tests_of)
                }) else {
                    return Some(format!(
                        "filter-differs: {} ran other tests",
                        planned.target
                    ));
                };
                resting.push((execution, ["whole"].as_slice()));
            }
            resting
        }
        other => return Some(format!("a carried record says {other}")),
    };
    resting
        .into_iter()
        .find_map(|(execution, enough)| execution_fails(execution, enough, held))
}

/// Why one execution would not do on this tree what it did on its own: its record does not reach far enough, its skeleton moved, or a body it entered changed or is no longer sealed.
fn execution_fails(execution: &Execution, enough: &[&str], held: &Held<'_>) -> Option<String> {
    if !enough.contains(&execution.completeness.as_str()) {
        return Some(format!("entry-incomplete: {}", execution.completeness));
    }
    if execution.skeleton != held.tree {
        return Some("skeleton-changed".to_owned());
    }
    for entered in &execution.entered {
        match held.bodies.get(&entered.item) {
            Some((now, _)) if *now != entered.body_digest => {
                return Some(format!("item-changed: {}", entered.item));
            }
            Some((_, false)) => return Some(format!("unsealed: {}", entered.item)),
            Some((_, true)) => {}
            None => return Some(format!("item-changed: {} is gone", entered.item)),
        }
    }
    None
}
