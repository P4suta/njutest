// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;

use syn::parse::Parser as _;
use syn::visit::Visit;

#[derive(Clone, Copy)]
struct Possibility {
    yes: bool,
    no: bool,
}

impl Possibility {
    const FALSE: Self = Self {
        yes: false,
        no: true,
    };
    const UNKNOWN: Self = Self {
        yes: true,
        no: true,
    };
}

fn possible(meta: &syn::Meta) -> Possibility {
    match meta {
        syn::Meta::Path(path) if path.is_ident("test") => Possibility::FALSE,
        syn::Meta::NameValue(value) if value.path.is_ident("feature") => {
            if matches!(&value.value, syn::Expr::Lit(literal) if matches!(&literal.lit, syn::Lit::Str(feature) if feature.value() == "testkit"))
            {
                Possibility::FALSE
            } else {
                Possibility::UNKNOWN
            }
        }
        syn::Meta::List(list) if list.path.is_ident("not") => {
            let Ok(inside) = list.parse_args::<syn::Meta>() else {
                return Possibility::UNKNOWN;
            };
            let inside = possible(&inside);
            Possibility {
                yes: inside.no,
                no: inside.yes,
            }
        }
        syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
            let Ok(inside) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return Possibility::UNKNOWN;
            };
            if list.path.is_ident("all") {
                Possibility {
                    yes: inside.iter().all(|one| possible(one).yes),
                    no: inside.iter().any(|one| possible(one).no),
                }
            } else {
                Possibility {
                    yes: inside.iter().any(|one| possible(one).yes),
                    no: inside.iter().all(|one| possible(one).no),
                }
            }
        }
        _ => Possibility::UNKNOWN,
    }
}

fn cfg_restricts(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    list.path.is_ident("cfg")
        && list
            .parse_args::<syn::Meta>()
            .is_ok_and(|predicate| !possible(&predicate).yes)
}

fn attribute_restricts(attribute: &syn::Attribute) -> bool {
    if cfg_restricts(&attribute.meta) {
        return true;
    }
    let syn::Meta::List(list) = &attribute.meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    let Ok(arguments) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        return false;
    };
    let mut arguments = arguments.iter();
    let Some(condition) = arguments.next() else {
        return false;
    };
    !possible(condition).no && arguments.any(cfg_restricts)
}

fn can_ship(attributes: &[syn::Attribute]) -> bool {
    !attributes.iter().any(attribute_restricts)
}

fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(one) => &one.attrs,
        syn::Item::Enum(one) => &one.attrs,
        syn::Item::ExternCrate(one) => &one.attrs,
        syn::Item::Fn(one) => &one.attrs,
        syn::Item::ForeignMod(one) => &one.attrs,
        syn::Item::Impl(one) => &one.attrs,
        syn::Item::Macro(one) => &one.attrs,
        syn::Item::Mod(one) => &one.attrs,
        syn::Item::Static(one) => &one.attrs,
        syn::Item::Struct(one) => &one.attrs,
        syn::Item::Trait(one) => &one.attrs,
        syn::Item::TraitAlias(one) => &one.attrs,
        syn::Item::Type(one) => &one.attrs,
        syn::Item::Union(one) => &one.attrs,
        syn::Item::Use(one) => &one.attrs,
        _ => &[],
    }
}

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
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !self.production || can_ship(item_attributes(item)) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes = match item {
            syn::ImplItem::Const(one) => &one.attrs,
            syn::ImplItem::Fn(one) => &one.attrs,
            syn::ImplItem::Type(one) => &one.attrs,
            syn::ImplItem::Macro(one) => &one.attrs,
            _ => return,
        };
        if !self.production || can_ship(attributes) {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attributes = match item {
            syn::TraitItem::Const(one) => &one.attrs,
            syn::TraitItem::Fn(one) => &one.attrs,
            syn::TraitItem::Type(one) => &one.attrs,
            syn::TraitItem::Macro(one) => &one.attrs,
            _ => return,
        };
        if !self.production || can_ship(attributes) {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if !self.production || can_ship(&local.attrs) {
            syn::visit::visit_local(self, local);
        }
    }

    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        if !self.production || can_ship(expression_attributes(expression)) {
            syn::visit::visit_expr(self, expression);
        }
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if !self.production || can_ship(&arm.attrs) {
            syn::visit::visit_arm(self, arm);
        }
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
    };
    scanner.visit_file(&parsed);
    Ok(scanner.facts)
}
