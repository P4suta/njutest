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
        syn::UseTree::Path(path) => used(&path.tree, Some(root.unwrap_or(&path.ident)), lists),
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
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, last)| last)
        .chars()
        .count();
    Some(LineColumn { line, column })
}

/// The page this audit reads its lists from, as this commit holds it.
const PAGE: &str = include_str!("../../../docs/engine/carry.md");

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

/// Every item the carry evidence names, joined to the body span the guards' record gives it.
fn cataloged(skeletons: &serde_json::Value, touched: &serde_json::Value) -> Vec<Cataloged> {
    let spans: std::collections::BTreeMap<u64, (String, std::ops::Range<usize>)> = touched
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let index = item.get("index")?.as_u64()?;
            let path = item.get("path")?.as_str()?.to_owned();
            let body = item.get("body")?;
            let start = offset(body.get("start"))?;
            let end = offset(body.get("end"))?;
            Some((index, (path, start..end)))
        })
        .collect();
    skeletons
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let index = item.get("index")?.as_u64()?;
            let (path, body) = spans.get(&index)?.clone();
            let named = item.get("item")?;
            let claimed = (
                named.get("path")?.as_str()?.to_owned(),
                named.get("ordinal")?.as_u64()?,
            );
            Some(Cataloged {
                index,
                path,
                claimed,
                name: item.get("name")?.as_str()?.to_owned(),
                body,
                digest: item.get("body_digest")?.as_str()?.to_owned(),
                sealed: item.get("sealed")?.as_bool()?,
            })
        })
        .collect()
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

/// A byte offset the evidence keeps, where it is one this platform can index by.
fn offset(value: Option<&serde_json::Value>) -> Option<usize> {
    match usize::try_from(value?.as_u64()?) {
        Ok(offset) => Some(offset),
        Err(_too_wide) => None,
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

/// The carry evidence against the tree the run measured: every body digest, every body the run calls sealed, every `$root` entry's placeholder rendering, and every skeleton's fold.
pub(super) fn layer(
    report: &super::Report,
    evidence: &super::CheckedEvidence<'_>,
    audit: &mut super::Audit,
) {
    let mut notes = super::Notes::on(audit, super::Layer::Carry);
    let Some(skeletons) = evidence.skeletons.as_ref() else {
        return;
    };
    let Some(root) = evidence.root else {
        notes.unaudited(
            "root",
            "no --root names the tree the run measured, so its carry evidence is not read again"
                .to_owned(),
        );
        return;
    };
    let Some(touched) = evidence.touched.as_ref() else {
        notes.unaudited(
            "touched",
            "the run kept no record of its items' body spans, so no body can be read again"
                .to_owned(),
        );
        return;
    };
    let page = match PageLists::read(PAGE) {
        Ok(page) => page,
        Err(refusal) => {
            notes.unaudited("page", format!("{refusal}, so no list is read"));
            return;
        }
    };
    let measured: std::collections::BTreeMap<&str, &str> = report
        .mutants
        .iter()
        .map(|row| (row.path.as_str(), row.source_digest.as_str()))
        .collect();
    let items = cataloged(skeletons, touched);
    let mut read = Tree::new(root, &measured);
    for item in &items {
        bodies(item, &mut read, (page.lists(), skeletons), &mut notes);
    }
    refs(&items, &mut notes);
    skeleton_folds(skeletons, &items, &mut read, &mut notes);
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
    (lists, skeletons): (Lists<'_>, &serde_json::Value),
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
fn read_by_the_compiler(path: &str, skeletons: &serde_json::Value) -> bool {
    let named = format!("$root/{path}");
    skeletons
        .get("units")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|unit| {
            unit.get("entries")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|entries| entries.contains_key(&named))
        })
        .filter_map(|unit| unit.get("kind").and_then(serde_json::Value::as_str))
        .any(|kind| {
            kind.split(',')
                .any(|one| one == "proc-macro" || one == "custom-build")
        })
}

/// Every file read by any unit that read `path` that parses as a whole Rust file, whatever its extension, or `None` where one of them cannot be read: a `$target` entry, or a `$root` one --root does not hold.
fn unit_files(
    path: &str,
    skeletons: &serde_json::Value,
    tree: &mut Tree<'_>,
) -> Option<Vec<syn::File>> {
    let named = format!("$root/{path}");
    let mut files = Vec::new();
    for unit in skeletons
        .get("units")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(entries) = unit.get("entries").and_then(serde_json::Value::as_object) else {
            continue;
        };
        if !entries.contains_key(&named) {
            continue;
        }
        for entry in entries.keys() {
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
    skeletons: &serde_json::Value,
    items: &[Cataloged],
    tree: &mut Tree<'_>,
    notes: &mut super::Notes<'_>,
) {
    for unit in skeletons
        .get("units")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = format!(
            "{}/{}",
            unit.get("package")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            unit.get("target")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        );
        let Some(entries) = unit.get("entries").and_then(serde_json::Value::as_object) else {
            notes.unaudited(
                &name,
                "the unit keeps no entries, so its skeleton is not re-folded".to_owned(),
            );
            continue;
        };
        let mut folded = String::new();
        for (entry, digest) in entries {
            folded.push_str(entry);
            folded.push('\0');
            folded.push_str(digest.as_str().unwrap_or_default());
            folded.push('\n');
        }
        if unit.get("skeleton").and_then(serde_json::Value::as_str)
            != Some(sha256(folded.as_bytes()).as_str())
        {
            notes.violated(
                &name,
                "the unit's skeleton is not the fold of the entries it keeps".to_owned(),
            );
        }
        for (entry, digest) in entries {
            let Some(path) = entry.strip_prefix("$root/") else {
                continue;
            };
            let Some((text, _)) = tree.text(path) else {
                continue;
            };
            let rendered = rendering(entry, path, &text, items);
            if digest.as_str() != Some(sha256(rendered.as_bytes()).as_str()) {
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

/// `text` with every sealed body of the file at `path` replaced by the placeholder naming it, as the page defines it.
fn rendering(entry: &str, path: &str, text: &str, items: &[Cataloged]) -> String {
    let mut ordered: Vec<&Cataloged> = items.iter().filter(|item| item.path == path).collect();
    ordered.sort_by_key(|item| item.index);
    let mut rendered = String::new();
    let mut at = 0_usize;
    for (ordinal, item) in ordered.iter().enumerate() {
        if !item.sealed {
            continue;
        }
        rendered.push_str(text.get(at..item.body.start).unwrap_or_default());
        let body = text.get(item.body.clone()).unwrap_or_default();
        let last = body.rsplit('\n').next().unwrap_or_default();
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
    rendered.push_str(text.get(at..).unwrap_or_default());
    rendered
}
