// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Uses outside native validation that prevent a linked const fn from losing its const.

use syn::visit::{self, Visit};

/// Whether the complete source, including suppressed code, may evaluate a linked const fn outside validation.
pub(super) fn uses(file: &syn::File) -> bool {
    let mut uses = Uses::default();
    uses.visit_file(file);
    uses.found
}

#[derive(Default)]
struct Uses {
    conditional: bool,
    found: bool,
}

impl Uses {
    fn within(&mut self, attrs: &[syn::Attribute], walk: impl FnOnce(&mut Self)) {
        let outer = self.conditional;
        self.conditional |= attrs.iter().any(conditional);
        walk(self);
        self.conditional = outer;
    }

    fn early(&mut self, expression: &syn::Expr) {
        self.found |= self.conditional && crate::skeleton::computes(expression);
    }
}

/// Native all-target validation compiles cfg(test); every other condition may hide code it never compiles.
fn conditional(attribute: &syn::Attribute) -> bool {
    if attribute.path().is_ident("cfg_attr") {
        return true;
    }
    if !attribute.path().is_ident("cfg") {
        return false;
    }
    match &attribute.meta {
        syn::Meta::List(list) => match list.parse_args::<syn::Path>() {
            Ok(path) => !path.is_ident("test"),
            Err(_not_test_alone) => true,
        },
        syn::Meta::Path(_) | syn::Meta::NameValue(_) => true,
    }
}

/// Documentation supplied through `cfg_attr` is still source rustdoc may compile outside validation.
fn documented(attribute: &syn::Attribute) -> bool {
    if attribute.path().is_ident("doc") {
        return crate::skeleton::documents_code(attribute);
    }
    if !attribute.path().is_ident("cfg_attr") {
        return false;
    }
    let syn::Meta::List(list) = &attribute.meta else {
        return true;
    };
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(applied) => applied.iter().skip(1).any(|meta| {
            documented(&syn::Attribute {
                pound_token: attribute.pound_token,
                style: attribute.style,
                bracket_token: attribute.bracket_token,
                meta: meta.clone(),
            })
        }),
        Err(_unread) => true,
    }
}

/// A standard macro's arguments may declare documentation or apply an opaque attribute too.
fn attributed(tokens: &proc_macro2::TokenStream) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        proc_macro2::TokenTree::Punct(punctuation) => punctuation.as_char() == '#',
        proc_macro2::TokenTree::Group(group) => attributed(&group.stream()),
        proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Literal(_) => false,
    })
}

/// An import that may replace a listed standard macro with opaque expansion code.
fn imported(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Path(path) => imported(&path.tree),
        syn::UseTree::Name(name) => replaced(&name.ident),
        syn::UseTree::Rename(rename) => replaced(&rename.rename),
        syn::UseTree::Group(group) => group.items.iter().any(imported),
        syn::UseTree::Glob(_) => true,
    }
}

/// A binding whose name would make an opaque expansion look like a listed standard macro.
fn replaced(name: &syn::Ident) -> bool {
    let name = name.to_string();
    crate::skeleton::SEALABLE_MACROS.contains(&name.as_str())
        || crate::skeleton::STANDARD_ROOTS.contains(&name.as_str())
}

impl<'ast> Visit<'ast> for Uses {
    fn visit_file(&mut self, node: &'ast syn::File) {
        self.within(&node.attrs, |this| visit::visit_file(this, node));
    }

    fn visit_item(&mut self, node: &'ast syn::Item) {
        self.within(super::shape::item_attrs(node), |this| {
            this.found |= matches!(node, syn::Item::Mod(module) if crate::skeleton::STANDARD_ROOTS.contains(&module.ident.to_string().as_str()));
            if this.conditional
                && matches!(node, syn::Item::Mod(module) if module.content.is_none())
            {
                this.found = true;
            }
            visit::visit_item(this, node);
        });
    }

    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        let attrs: &[syn::Attribute] = match node {
            syn::ImplItem::Const(item) => &item.attrs,
            syn::ImplItem::Fn(item) => &item.attrs,
            syn::ImplItem::Type(item) => &item.attrs,
            syn::ImplItem::Macro(item) => &item.attrs,
            _ => &[],
        };
        self.within(attrs, |this| visit::visit_impl_item(this, node));
    }

    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        let attrs: &[syn::Attribute] = match node {
            syn::TraitItem::Const(item) => &item.attrs,
            syn::TraitItem::Fn(item) => &item.attrs,
            syn::TraitItem::Type(item) => &item.attrs,
            syn::TraitItem::Macro(item) => &item.attrs,
            _ => &[],
        };
        self.within(attrs, |this| visit::visit_trait_item(this, node));
    }

    fn visit_foreign_item(&mut self, node: &'ast syn::ForeignItem) {
        let attrs: &[syn::Attribute] = match node {
            syn::ForeignItem::Fn(item) => &item.attrs,
            syn::ForeignItem::Static(item) => &item.attrs,
            syn::ForeignItem::Type(item) => &item.attrs,
            syn::ForeignItem::Macro(item) => &item.attrs,
            _ => &[],
        };
        self.within(attrs, |this| visit::visit_foreign_item(this, node));
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        self.within(super::shape::expr_attrs(node), |this| {
            match node {
                syn::Expr::Const(_) => this.early(node),
                syn::Expr::Repeat(repeat) => this.early(&repeat.len),
                _ => {}
            }
            visit::visit_expr(this, node);
        });
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        self.within(&node.attrs, |this| visit::visit_local(this, node));
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.within(&node.attrs, |this| visit::visit_arm(this, node));
    }

    fn visit_field(&mut self, node: &'ast syn::Field) {
        self.within(&node.attrs, |this| visit::visit_field(this, node));
    }

    fn visit_signature(&mut self, node: &'ast syn::Signature) {
        self.found |= self.conditional && node.constness.is_some();
        visit::visit_signature(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        self.found |= documented(node) || crate::skeleton::attribute(node).is_some();
        visit::visit_attribute(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.found |= self.conditional
            || crate::skeleton::invoked(&node.path, &node.tokens).is_some()
            || attributed(&node.tokens);
        visit::visit_macro(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let standard = matches!(&node.tree, syn::UseTree::Path(path) if crate::skeleton::STANDARD_ROOTS.contains(&path.ident.to_string().as_str()));
        self.found |= !standard && imported(&node.tree);
        visit::visit_item_use(self, node);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        let name = match &node.rename {
            Some((_, renamed)) => renamed,
            None => &node.ident,
        };
        self.found |= crate::skeleton::STANDARD_ROOTS.contains(&name.to_string().as_str());
        visit::visit_item_extern_crate(self, node);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.early(&node.expr);
        visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.early(&node.expr);
        visit::visit_item_static(self, node);
    }

    fn visit_impl_item_const(&mut self, node: &'ast syn::ImplItemConst) {
        self.early(&node.expr);
        visit::visit_impl_item_const(self, node);
    }

    fn visit_trait_item_const(&mut self, node: &'ast syn::TraitItemConst) {
        if let Some((_, value)) = &node.default {
            self.early(value);
        }
        visit::visit_trait_item_const(self, node);
    }

    fn visit_type_array(&mut self, node: &'ast syn::TypeArray) {
        self.early(&node.len);
        visit::visit_type_array(self, node);
    }

    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        self.within(&node.attrs, |this| {
            if let Some((_, value)) = &node.discriminant {
                this.early(value);
            }
            visit::visit_variant(this, node);
        });
    }

    fn visit_const_param(&mut self, node: &'ast syn::ConstParam) {
        self.within(&node.attrs, |this| {
            if let Some((_, value)) = &node.default {
                this.early(value);
            }
            visit::visit_const_param(this, node);
        });
    }

    fn visit_generic_argument(&mut self, node: &'ast syn::GenericArgument) {
        if let syn::GenericArgument::Const(value) = node {
            self.early(value);
        }
        visit::visit_generic_argument(self, node);
    }
}
