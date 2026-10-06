// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The modules `unsafe` code may be written in, named once so the rule refusing it elsewhere and the strict-conversion policy read one list (ADR 0037 decision 8).

use syn::visit::Visit;

use super::{Finding, Kind, macro_tokens_name};

/// Every module that calls a platform through a foreign boundary, and the only files `unsafe` code or an expectation of it may be written in.
pub(super) const MODULES: [&str; 4] = [
    "crates/rust-mutants/src/capdir/windows.rs",
    "crates/njutest-process/src/unix.rs",
    "crates/njutest-process/src/windows.rs",
    "crates/rust-mutants/src/tempowner/lock.rs",
];

/// Whether `file` is held to the rule, which is every file the gate reads but the named modules.
pub(super) fn held(file: &str) -> bool {
    !MODULES.contains(&file)
}

/// Every place `parsed` writes `unsafe` or lowers the `unsafe_code` lint, each at its line.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut scan = Unsafe {
        file,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Unsafe<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

impl Unsafe<'_> {
    fn note(&mut self, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind: Kind::UnsafeOutsideFfi,
            file: self.file.to_owned(),
            line: span.start().line,
        });
    }
}

impl<'ast> Visit<'ast> for Unsafe<'_> {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.note(node.unsafe_token.span);
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_signature(&mut self, node: &'ast syn::Signature) {
        match &node.safety {
            syn::Safety::Unsafe(token) => self.note(token.span),
            syn::Safety::Safe(_) | syn::Safety::Default => {}
        }
        syn::visit::visit_signature(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let Some(token) = &node.unsafety {
            self.note(token.span);
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if let Some(token) = &node.unsafety {
            self.note(token.span);
        }
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        if let Some(token) = &node.unsafety {
            self.note(token.span);
        }
        syn::visit::visit_item_foreign_mod(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("unsafe") || lowers(&node.meta) {
            self.note(node.pound_token.span);
        }
        syn::visit::visit_attribute(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for span in unsafe_tokens(&node.tokens) {
            self.note(span);
        }
        syn::visit::visit_macro(self, node);
    }
}

/// Whether `meta` lets `unsafe` code compile where the workspace denies it: an `expect`, `allow` or `warn` of `unsafe_code`, directly or under `cfg_attr`.
fn lowers(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if ["expect", "allow", "warn"]
        .iter()
        .any(|level| list.path.is_ident(level))
    {
        return macro_tokens_name(&list.tokens, "unsafe_code");
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested.iter().skip(1).any(lowers),
        Err(_unparsed) => macro_tokens_name(&list.tokens, "unsafe_code"),
    }
}

/// Where `tokens` spell the keyword `unsafe`, at any depth, which a macro turns into code only after this gate has read it.
fn unsafe_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    for token in tokens.clone() {
        match token {
            proc_macro2::TokenTree::Ident(ident) if ident == "unsafe" => found.push(ident.span()),
            proc_macro2::TokenTree::Group(group) => found.extend(unsafe_tokens(&group.stream())),
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}
