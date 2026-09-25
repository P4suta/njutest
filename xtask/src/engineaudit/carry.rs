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
            syn::Item::Fn(function) => self.candidate(
                &function.block,
                function.sig.constness.is_some(),
                &function.attrs,
            ),
            syn::Item::Mod(module) => {
                if let Some((_, inner)) = &module.content {
                    self.within(&module.attrs, |search| search.items(inner));
                }
            }
            syn::Item::Impl(block) => self.within(&block.attrs, |search| {
                for member in &block.items {
                    if let syn::ImplItem::Fn(method) = member {
                        search.candidate(
                            &method.block,
                            method.sig.constness.is_some(),
                            &method.attrs,
                        );
                    }
                }
            }),
            syn::Item::Trait(declared) => self.within(&declared.attrs, |search| {
                for member in &declared.items {
                    if let syn::TraitItem::Fn(method) = member
                        && let Some(block) = &method.default
                    {
                        search.candidate(block, method.sig.constness.is_some(), &method.attrs);
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

    fn candidate(&mut self, block: &syn::Block, constness: bool, attributes: &[syn::Attribute]) {
        if block.brace_token.span.open().start() == self.start {
            let mut context = self.context.clone();
            context.extend(attributes.iter().cloned());
            self.found = Some(Located {
                block: block.clone(),
                constness,
                context,
            });
        }
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
                if delimited && !keyword(&named) && !lists.macros.iter().any(|one| *one == named) {
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
        (1, Some(name)) => name != "cfg_attr" && lists.attributes.iter().any(|one| *one == name),
        (_, Some(name)) => lists.tools.iter().any(|one| *one == name),
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

impl PageLists {
    /// The lists of the page `text`, or the name of the first block it lacks.
    ///
    /// # Errors
    /// The name of a fenced block the page does not hold.
    pub fn read(text: &str) -> Result<Self, &'static str> {
        let block = |name: &'static str| -> Result<Vec<String>, &'static str> {
            let (_, after) = text.split_once(&format!("```{name}\n")).ok_or(name)?;
            let (body, _) = after.split_once("```").ok_or(name)?;
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
