// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failed compilations keep the original process stderr beside compiler messages.

use syn::visit::Visit as _;

use super::super::cfg_conditions::{CfgScope, CfgWorld};
use super::{Finding, Kind, item_attributes};

pub(super) const REMEDY: &str = "render the actual compilation with first_error_with_stderr(messages, stderr). A message-only diagnostic or invented empty capture discards compiler-wrapper failures before the error boundary can report them";

pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    if file
        .split('/')
        .take_while(|part| *part != "src")
        .any(|part| part == "tests")
    {
        return Vec::new();
    }
    let mut scan = Scan {
        file,
        conditions: CfgScope::new(CfgWorld::Production),
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Scan<'a> {
    file: &'a str,
    conditions: CfgScope,
    found: Vec<Finding>,
}

impl Scan<'_> {
    fn within(&mut self, attributes: &[syn::Attribute], walk: impl FnOnce(&mut Self)) {
        let mark = self.conditions.mark();
        for attribute in attributes {
            self.conditions.push(attribute);
        }
        if self.conditions.possible() {
            walk(self);
        }
        self.conditions.truncate(mark);
    }

    fn note(&mut self, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind: Kind::DiscardedCompilerStderr,
            file: self.file.to_owned(),
            line: span.start().line,
        });
    }

    fn tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        let trees: Vec<_> = tokens.clone().into_iter().collect();
        for tree in &trees {
            match tree {
                proc_macro2::TokenTree::Ident(name) if name == "first_error_of" => {
                    self.note(name.span())
                }
                proc_macro2::TokenTree::Group(group) => self.tokens(&group.stream()),
                proc_macro2::TokenTree::Ident(_)
                | proc_macro2::TokenTree::Punct(_)
                | proc_macro2::TokenTree::Literal(_) => {}
            }
        }
        for pair in trees.windows(2) {
            if let [
                proc_macro2::TokenTree::Ident(name),
                proc_macro2::TokenTree::Group(args),
            ] = pair
                && name == "first_error_with_stderr"
                && empty_tail(&args.stream())
            {
                self.note(name.span());
            }
        }
    }
}

fn empty_tail(tokens: &proc_macro2::TokenStream) -> bool {
    let trees: Vec<_> = tokens.clone().into_iter().collect();
    matches!(trees.as_slice(), [.., proc_macro2::TokenTree::Punct(comma), proc_macro2::TokenTree::Punct(reference), proc_macro2::TokenTree::Group(array)] if comma.as_char() == ',' && reference.as_char() == '&' && array.delimiter() == proc_macro2::Delimiter::Bracket && array.stream().is_empty())
}

fn empty_capture(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Reference(reference) => empty_capture(&reference.expr),
        syn::Expr::Array(array) => array.elems.is_empty(),
        syn::Expr::Paren(group) => empty_capture(&group.expr),
        syn::Expr::Group(group) => empty_capture(&group.expr),
        _ => false,
    }
}

impl<'ast> syn::visit::Visit<'ast> for Scan<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.within(&file.attrs, |scan| syn::visit::visit_file(scan, file));
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.within(item_attributes(item), |scan| {
            syn::visit::visit_item(scan, item)
        });
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::ImplItem::Const(one) => &one.attrs,
            syn::ImplItem::Fn(one) => &one.attrs,
            syn::ImplItem::Type(one) => &one.attrs,
            syn::ImplItem::Macro(one) => &one.attrs,
            _ => &[],
        };
        self.within(attributes, |scan| syn::visit::visit_impl_item(scan, item));
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::TraitItem::Const(one) => &one.attrs,
            syn::TraitItem::Fn(one) => &one.attrs,
            syn::TraitItem::Type(one) => &one.attrs,
            syn::TraitItem::Macro(one) => &one.attrs,
            _ => &[],
        };
        self.within(attributes, |scan| syn::visit::visit_trait_item(scan, item));
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if let Some(last) = path.path.segments.last()
            && last.ident == "first_error_of"
        {
            self.note(last.ident.span());
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for span in super::imported_function_spans(&item.tree, "first_error_of") {
            self.note(span);
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && let Some(last) = path.path.segments.last()
            && last.ident == "first_error_with_stderr"
            && call.args.last().is_some_and(empty_capture)
        {
            self.note(last.ident.span());
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.tokens(&invocation.tokens);
    }
}
