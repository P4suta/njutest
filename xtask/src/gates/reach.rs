// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;

use syn::parse::Parser as _;
use syn::visit::Visit;

use super::super::cfg_conditions::{CfgScope, CfgWorld, item_attributes};

fn expression_attributes(expression: &syn::Expr) -> &[syn::Attribute] {
    match expression {
        syn::Expr::Array(one) => &one.attrs,
        syn::Expr::Assign(one) => &one.attrs,
        syn::Expr::Async(one) => &one.attrs,
        syn::Expr::Await(one) => &one.attrs,
        syn::Expr::Binary(one) => &one.attrs,
        syn::Expr::Block(one) => &one.attrs,
        syn::Expr::Break(one) => &one.attrs,
        syn::Expr::Call(one) => &one.attrs,
        syn::Expr::Cast(one) => &one.attrs,
        syn::Expr::Closure(one) => &one.attrs,
        syn::Expr::Const(one) => &one.attrs,
        syn::Expr::Continue(one) => &one.attrs,
        syn::Expr::Field(one) => &one.attrs,
        syn::Expr::ForLoop(one) => &one.attrs,
        syn::Expr::Group(one) => &one.attrs,
        syn::Expr::If(one) => &one.attrs,
        syn::Expr::Index(one) => &one.attrs,
        syn::Expr::Infer(one) => &one.attrs,
        syn::Expr::Let(one) => &one.attrs,
        syn::Expr::Lit(one) => &one.attrs,
        syn::Expr::Loop(one) => &one.attrs,
        syn::Expr::Macro(one) => &one.attrs,
        syn::Expr::Match(one) => &one.attrs,
        syn::Expr::MethodCall(one) => &one.attrs,
        syn::Expr::Paren(one) => &one.attrs,
        syn::Expr::Path(one) => &one.attrs,
        syn::Expr::Range(one) => &one.attrs,
        syn::Expr::RawAddr(one) => &one.attrs,
        syn::Expr::Reference(one) => &one.attrs,
        syn::Expr::Repeat(one) => &one.attrs,
        syn::Expr::Return(one) => &one.attrs,
        syn::Expr::Struct(one) => &one.attrs,
        syn::Expr::Try(one) => &one.attrs,
        syn::Expr::TryBlock(one) => &one.attrs,
        syn::Expr::Tuple(one) => &one.attrs,
        syn::Expr::Unary(one) => &one.attrs,
        syn::Expr::Unsafe(one) => &one.attrs,
        syn::Expr::While(one) => &one.attrs,
        syn::Expr::Yield(one) => &one.attrs,
        _ => &[],
    }
}

#[derive(Default)]
pub(super) struct Facts {
    pub(super) public: BTreeSet<String>,
    pub(super) referenced: BTreeSet<String>,
}

struct Scanner {
    production: bool,
    facts: Facts,
    active_cfg: CfgScope,
}

impl Scanner {
    fn visit_scoped(&mut self, attributes: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        if !self.production {
            visit(self);
            return;
        }
        let previous = self.active_cfg.mark();
        let mut changed = false;
        for attribute in attributes {
            changed |= self.active_cfg.push(attribute);
        }
        if !changed || self.active_cfg.possible() {
            visit(self);
        }
        self.active_cfg.truncate(previous);
    }
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.visit_scoped(&file.attrs, |this| syn::visit::visit_file(this, file));
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.visit_scoped(item_attributes(item), |this| {
            syn::visit::visit_item(this, item);
        });
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes = match item {
            syn::ImplItem::Const(one) => &one.attrs,
            syn::ImplItem::Fn(one) => &one.attrs,
            syn::ImplItem::Type(one) => &one.attrs,
            syn::ImplItem::Macro(one) => &one.attrs,
            _ => return,
        };
        self.visit_scoped(attributes, |this| syn::visit::visit_impl_item(this, item));
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attributes = match item {
            syn::TraitItem::Const(one) => &one.attrs,
            syn::TraitItem::Fn(one) => &one.attrs,
            syn::TraitItem::Type(one) => &one.attrs,
            syn::TraitItem::Macro(one) => &one.attrs,
            _ => return,
        };
        self.visit_scoped(attributes, |this| syn::visit::visit_trait_item(this, item));
    }

    fn visit_foreign_item(&mut self, item: &'ast syn::ForeignItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::ForeignItem::Fn(one) => &one.attrs,
            syn::ForeignItem::Static(one) => &one.attrs,
            syn::ForeignItem::Type(one) => &one.attrs,
            syn::ForeignItem::Macro(one) => &one.attrs,
            _ => &[],
        };
        self.visit_scoped(attributes, |this| {
            syn::visit::visit_foreign_item(this, item);
        });
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        self.visit_scoped(&field.attrs, |this| syn::visit::visit_field(this, field));
    }

    fn visit_variant(&mut self, variant: &'ast syn::Variant) {
        self.visit_scoped(&variant.attrs, |this| {
            syn::visit::visit_variant(this, variant);
        });
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        self.visit_scoped(&local.attrs, |this| syn::visit::visit_local(this, local));
    }

    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        self.visit_scoped(expression_attributes(expression), |this| {
            syn::visit::visit_expr(this, expression);
        });
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.visit_scoped(&arm.attrs, |this| syn::visit::visit_arm(this, arm));
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.facts.public.insert(item.sig.ident.to_string());
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.facts.public.insert(item.sig.ident.to_string());
        }
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if let Some(segment) = expression.path.segments.last() {
            self.facts.referenced.insert(segment.ident.to_string());
        }
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        self.facts.referenced.insert(expression.method.to_string());
        syn::visit::visit_expr_method_call(self, expression);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let Some(name) = mac.path.segments.last() else {
            return;
        };
        if !matches!(
            name.ident.to_string().as_str(),
            "vec"
                | "format"
                | "format_args"
                | "write"
                | "writeln"
                | "println"
                | "eprintln"
                | "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "debug_assert_eq"
                | "debug_assert_ne"
                | "matches"
                | "dbg"
                | "panic"
        ) {
            return;
        }
        let expressions =
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                .parse2(mac.tokens.clone());
        match expressions {
            Ok(expressions) => {
                for expression in &expressions {
                    self.visit_expr(expression);
                }
            }
            Err(_not_a_list) => {
                let Ok(expression) = syn::parse2::<syn::Expr>(mac.tokens.clone()) else {
                    return;
                };
                self.visit_expr(&expression);
            }
        }
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        let syn::Meta::List(list) = &attribute.meta else {
            return;
        };
        if !list.path.is_ident("command") {
            return;
        }
        let Ok(arguments) = list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) else {
            return;
        };
        for argument in &arguments {
            if let syn::Meta::NameValue(value) = argument {
                self.visit_expr(&value.value);
            }
        }
    }
}

pub(super) fn facts(source: &str, production: bool) -> syn::Result<Facts> {
    let parsed = syn::parse_file(source)?;
    let mut scanner = Scanner {
        production,
        facts: Facts::default(),
        active_cfg: CfgScope::new(CfgWorld::Production),
    };
    scanner.visit_file(&parsed);
    Ok(scanner.facts)
}
