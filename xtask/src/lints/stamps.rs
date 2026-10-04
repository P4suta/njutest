// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A file's stamp compared only through `njutest_fixture_tree::settled::Taken`, which asks whether it had settled when it was taken.

use std::collections::BTreeSet;

use syn::visit::Visit;

use super::{Finding, Kind};

/// Why a comparable stamp or a stray change time is refused, in the words a person needs to fix it.
pub(super) const REMEDY: &str = "hold the stamp in `njutest_fixture_tree::settled::Taken` and \
    reuse what it stands for only where `Taken::holds` says so. A rewrite of the same length \
    within one tick of the filesystem's clock leaves every field of a stamp equal, and Linux \
    dates writes from a coarse clock, so two writes milliseconds apart carry the same change \
    time and a memo keyed by plain equality hands back the identity of bytes that are gone; \
    `holds` refuses a stamp whose newest time was not older than the moment it was taken by \
    `settled::GRANULARITY`. A type that implements `settled::Stamp` therefore derives and \
    implements no equality of its own, and a file's change time (`ctime`, `ctime_nsec`, \
    `st_ctime`, `st_ctime_nsec`, `ChangeTime`) is read only in a file that implements \
    `settled::Stamp`, or in the engine's Windows FFI that reads it for one";

/// The engine's Windows FFI module, which reads the change time the engine's stamp holds.
pub(super) const FFI: &str = "crates/rust-mutants/src/capdir/windows.rs";

/// The names under which a platform reports a file's change time.
const CHANGE_TIMES: [&str; 5] = [
    "ctime",
    "ctime_nsec",
    "st_ctime",
    "st_ctime_nsec",
    "ChangeTime",
];

/// Every stamp `parsed` makes comparable, and every change time it reads where it implements no stamp, each at its line.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut survey = Survey::default();
    survey.visit_file(parsed);
    let stray: &[usize] = if survey.stamps.is_empty() && file != FFI {
        &survey.change_times
    } else {
        &[]
    };
    survey
        .comparable
        .iter()
        .filter(|(name, _line)| survey.stamps.contains(name))
        .map(|(_name, line)| *line)
        .chain(stray.iter().copied())
        .map(|line| Finding {
            kind: Kind::RawStamp,
            file: file.to_owned(),
            line,
        })
        .collect()
}

/// What one file says about stamps: the types it makes stamps, those it makes comparable, and where it reads a change time.
#[derive(Default)]
struct Survey {
    stamps: BTreeSet<String>,
    comparable: Vec<(String, usize)>,
    change_times: Vec<usize>,
}

/// The last name of `path`, as a trait or derive is spelled however it was imported.
fn last(path: &syn::Path) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// Whether `name` is one of the traits that compare by equality.
fn equality(name: &str) -> bool {
    matches!(name, "PartialEq" | "Eq")
}

/// Whether `meta` derives an equality, directly or under `cfg_attr`.
fn derives_equality(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| {
                paths
                    .iter()
                    .any(|path| last(path).is_some_and(|name| equality(&name)))
            });
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| nested.iter().skip(1).any(derives_equality))
}

impl Survey {
    fn declared(&mut self, attrs: &[syn::Attribute], ident: &syn::Ident) {
        if attrs
            .iter()
            .any(|attribute| derives_equality(&attribute.meta))
        {
            self.comparable
                .push((ident.to_string(), ident.span().start().line));
        }
    }

    fn named(&mut self, ident: &syn::Ident) {
        if CHANGE_TIMES.iter().any(|name| ident == name) {
            self.change_times.push(ident.span().start().line);
        }
    }

    fn scan_tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        for tree in tokens.clone() {
            match tree {
                proc_macro2::TokenTree::Ident(ident) => self.named(&ident),
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(&group.stream()),
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Survey {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.declared(&item.attrs, &item.ident);
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.declared(&item.attrs, &item.ident);
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if let Some((path, _)) = &item.trait_
            && let syn::Type::Path(implementer) = &*item.self_ty
            && let (Some(trait_name), Some(name)) = (last(path), last(&implementer.path))
        {
            if trait_name == "Stamp" {
                self.stamps.insert(name);
            } else if equality(&trait_name) {
                self.comparable
                    .push((name, item.impl_token.span.start().line));
            }
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.named(&call.method);
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_member(&mut self, member: &'ast syn::Member) {
        if let syn::Member::Named(ident) = member {
            self.named(ident);
        }
        syn::visit::visit_member(self, member);
    }

    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        self.named(&segment.ident);
        syn::visit::visit_path_segment(self, segment);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.scan_tokens(&invocation.tokens);
        syn::visit::visit_macro(self, invocation);
    }
}
