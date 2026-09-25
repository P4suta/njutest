// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a body is sealed, read again from its source under ADR 0041 with this audit's own parser and nothing of the engine's.

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

/// The lists a body is held to, read from `docs/engine/carry.md` rather than from the engine.
#[derive(Debug, Clone, Copy)]
pub struct Lists<'a> {
    /// The standard macros that expand to no item.
    pub macros: &'a [&'a str],
    /// The attributes a sealed item may carry.
    pub attributes: &'a [&'a str],
    /// The crate roots a macro path may name.
    pub roots: &'a [&'a str],
}

/// Why a body is not sealed, in the order this audit looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Unsealed {
    /// A `const fn`, whose body can be evaluated where nothing enters it.
    Evaluated,
    /// A macro outside the list, or named through a path the list does not reach.
    Macro,
    /// An attribute outside the list.
    Attribute,
    /// An item declared inside the body.
    DeclaresItem,
    /// An inline `const` block.
    ConstBlock,
    /// A file of the unit declares or imports a macro under a listed name.
    Shadowed,
    /// A glob import from outside the standard roots, or a `#[macro_use] extern crate`, somewhere in the unit.
    ForeignGlob,
}

impl Unsealed {
    /// The word a report names it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Evaluated => "evaluated",
            Self::Macro => "macro",
            Self::Attribute => "attribute",
            Self::DeclaresItem => "declares-item",
            Self::ConstBlock => "const-block",
            Self::Shadowed => "shadowed",
            Self::ForeignGlob => "foreign-glob",
        }
    }
}

/// Whether the function `item` of a unit whose files are `unit` is sealed, and if not, the first reason.
///
/// # Errors
/// The first [`Unsealed`] reason, in declaration order.
pub fn sealing(item: &syn::ItemFn, unit: &[syn::File], lists: Lists<'_>) -> Result<(), Unsealed> {
    if item.sig.constness.is_some() {
        return Err(Unsealed::Evaluated);
    }
    let mut seen = Seen::default();
    for attribute in &item.attrs {
        seen.visit_attribute(attribute);
    }
    seen.visit_block(&item.block);
    let body = seen.finish(lists);
    let mut reasons = body;
    if unit.iter().any(|file| shadows(file, lists)) {
        reasons.push(Unsealed::Shadowed);
    }
    if unit.iter().any(|file| foreign_glob(file, lists)) {
        reasons.push(Unsealed::ForeignGlob);
    }
    reasons.sort_unstable();
    reasons.first().map_or(Ok(()), |first| Err(*first))
}

/// What the walk of one body found.
#[derive(Debug, Default)]
struct Seen {
    macros: Vec<syn::Path>,
    tokens: Vec<TokenStream>,
    attributes: Vec<syn::Path>,
    items: bool,
    const_block: bool,
}

impl Seen {
    fn finish(self, lists: Lists<'_>) -> Vec<Unsealed> {
        let mut reasons = Vec::new();
        let listed_path = |path: &syn::Path| listed(path, lists);
        if !self.macros.iter().all(listed_path)
            || !self
                .tokens
                .iter()
                .all(|tokens| invoked_only_listed(tokens, lists))
        {
            reasons.push(Unsealed::Macro);
        }
        if !self
            .attributes
            .iter()
            .all(|path| allowed_attribute(path, lists))
        {
            reasons.push(Unsealed::Attribute);
        }
        if self.items {
            reasons.push(Unsealed::DeclaresItem);
        }
        if self.const_block {
            reasons.push(Unsealed::ConstBlock);
        }
        reasons
    }
}

impl<'ast> Visit<'ast> for Seen {
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.macros.push(node.path.clone());
        self.tokens.push(node.tokens.clone());
    }

    fn visit_item(&mut self, _node: &'ast syn::Item) {
        self.items = true;
    }

    fn visit_expr_const(&mut self, _node: &'ast syn::ExprConst) {
        self.const_block = true;
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        self.attributes.push(node.path().clone());
    }
}

/// Whether `path` names a listed macro through a root the list reaches.
fn listed(path: &syn::Path, lists: Lists<'_>) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    let rooted = path.segments.len() == 1
        || path
            .segments
            .first()
            .is_some_and(|first| lists.roots.iter().any(|root| first.ident == root));
    rooted && lists.macros.iter().any(|name| last.ident == name)
}

/// Whether every `name!` inside `tokens`, at any depth, names a listed macro.
fn invoked_only_listed(tokens: &TokenStream, lists: Lists<'_>) -> bool {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    trees.iter().enumerate().all(|(at, tree)| match tree {
        TokenTree::Group(group) => invoked_only_listed(&group.stream(), lists),
        TokenTree::Punct(punct) if punct.as_char() == '!' => {
            match at.checked_sub(1).and_then(|before| trees.get(before)) {
                Some(TokenTree::Ident(name)) => {
                    let followed_by_group = matches!(
                        at.checked_add(1).and_then(|next| trees.get(next)),
                        Some(TokenTree::Group(_))
                    );
                    !followed_by_group || lists.macros.iter().any(|listed| name == listed)
                }
                Some(TokenTree::Group(_) | TokenTree::Punct(_) | TokenTree::Literal(_)) | None => {
                    true
                }
            }
        }
        TokenTree::Punct(_) | TokenTree::Ident(_) | TokenTree::Literal(_) => true,
    })
}

/// Whether `path` is an attribute a sealed item may carry.
fn allowed_attribute(path: &syn::Path, lists: Lists<'_>) -> bool {
    let spelled = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    lists.attributes.iter().any(|allowed| {
        allowed
            .strip_suffix("::*")
            .map_or(spelled == *allowed, |namespace| {
                spelled.starts_with(&format!("{namespace}::"))
            })
    })
}

/// Whether `file` declares or imports a macro under a listed name.
fn shadows(file: &syn::File, lists: Lists<'_>) -> bool {
    let mut found = Shadows {
        lists,
        shadowed: false,
    };
    found.visit_file(file);
    found.shadowed
}

struct Shadows<'a> {
    lists: Lists<'a>,
    shadowed: bool,
}

impl<'ast> Visit<'ast> for Shadows<'_> {
    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if node.mac.path.is_ident("macro_rules")
            && node
                .ident
                .as_ref()
                .is_some_and(|name| self.lists.macros.iter().any(|listed| name == listed))
        {
            self.shadowed = true;
        }
        syn::visit::visit_item_macro(self, node);
    }

    fn visit_use_name(&mut self, node: &'ast syn::UseName) {
        if self.lists.macros.iter().any(|listed| node.ident == listed) {
            self.shadowed = true;
        }
    }

    fn visit_use_rename(&mut self, node: &'ast syn::UseRename) {
        if self.lists.macros.iter().any(|listed| node.rename == listed) {
            self.shadowed = true;
        }
    }
}

/// Whether `file` glob-imports from outside the standard and local roots, or pulls macros in with `#[macro_use] extern crate`.
fn foreign_glob(file: &syn::File, lists: Lists<'_>) -> bool {
    let mut found = Globs {
        lists,
        foreign: false,
    };
    found.visit_file(file);
    found.foreign
}

struct Globs<'a> {
    lists: Lists<'a>,
    foreign: bool,
}

impl<'ast> Visit<'ast> for Globs<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if glob_from_outside(&node.tree, None, self.lists) {
            self.foreign = true;
        }
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        if node
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("macro_use"))
        {
            self.foreign = true;
        }
    }
}

/// Whether `tree`, under the root `root` already walked, ends in a glob whose root is outside the lists' roots and the local ones.
fn glob_from_outside(tree: &syn::UseTree, root: Option<&syn::Ident>, lists: Lists<'_>) -> bool {
    let local = |ident: &syn::Ident| ["self", "super", "crate"].iter().any(|one| ident == one);
    let reached = |ident: &syn::Ident| lists.roots.iter().any(|one| ident == one) || local(ident);
    match tree {
        syn::UseTree::Path(path) => {
            glob_from_outside(&path.tree, Some(root.unwrap_or(&path.ident)), lists)
        }
        syn::UseTree::Glob(_) => root.is_some_and(|root| !reached(root)),
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|one| glob_from_outside(one, root, lists)),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) => false,
    }
}
