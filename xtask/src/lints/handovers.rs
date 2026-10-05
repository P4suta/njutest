// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A claimed directory changes what it is declared to be only under the claim that holds it.

use std::collections::BTreeSet;

use syn::visit::Visit;

use super::{Finding, Kind};

/// Why a claim taken after a release is refused, in the words a person needs to fix it.
pub(super) const REMEDY: &str = "declare the directory under the claim that holds it: \
    `Owner::release_as_cache` writes the cache declaration before the lock closes. A claim \
    taken again after a release hands the directory over through a moment nobody holds it, and \
    whoever takes the lock then (a sweep's probe, a watcher waiting on it, another run) turns \
    the second claim into a refusal, while a collector reads the released directory as \
    abandoned; a published source graph was refused as owned by another process this way \
    (RM5006)";

/// The claims of `rust_mutants::tempowner`, each of which takes a directory's lock afresh.
const CLAIMS: [&str; 4] = ["claim", "claim_as", "claim_cache", "claim_cache_of"];

/// The module the claims are declared in.
const OWNER_MODULE: &str = "tempowner";

/// Whether `file` is held to the rule, which is every file but a suite that tests the claim protocol itself.
pub(super) fn held(file: &str) -> bool {
    !file.contains("/tests/")
}

/// Every claim of `parsed` that follows a release in the body of the same function, each at its line.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut names = Names::default();
    names.visit_file(parsed);
    let mut scan = Handovers {
        file,
        names,
        released: false,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

/// The names a file reaches the owner module and its claims by.
#[derive(Default)]
struct Names {
    modules: BTreeSet<String>,
    claims: BTreeSet<String>,
}

impl Names {
    fn imported(&mut self, tree: &syn::UseTree, under: bool) {
        match tree {
            syn::UseTree::Path(path) => {
                self.imported(&path.tree, under || path.ident == OWNER_MODULE);
            }
            syn::UseTree::Name(name) => self.named(&name.ident, &name.ident, under),
            syn::UseTree::Rename(rename) => self.named(&rename.ident, &rename.rename, under),
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.imported(item, under);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    fn named(&mut self, imported: &syn::Ident, local: &syn::Ident, under: bool) {
        if imported == OWNER_MODULE || (under && imported == "self") {
            self.modules.insert(local.to_string());
        } else if under && CLAIMS.iter().any(|claim| imported == claim) {
            self.claims.insert(local.to_string());
        }
    }

    /// Whether `path` names one of the claims, by its module or by the name it was imported under.
    fn claims(&self, path: &syn::Path) -> bool {
        let names: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        match names.as_slice() {
            [name] => self.claims.contains(name),
            [.., module, name] => {
                CLAIMS.contains(&name.as_str())
                    && (module == OWNER_MODULE || self.modules.contains(module))
            }
            [] => false,
        }
    }
}

impl<'ast> Visit<'ast> for Names {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.imported(&item.tree, false);
    }
}

/// One pass over a file, knowing within each function body whether a release has been written yet.
struct Handovers<'a> {
    file: &'a str,
    names: Names,
    released: bool,
    found: Vec<Finding>,
}

impl Handovers<'_> {
    fn body(&mut self, visit: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.released, false);
        visit(self);
        self.released = outer;
    }
}

impl<'ast> Visit<'ast> for Handovers<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.body(|scan| syn::visit::visit_item_fn(scan, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.body(|scan| syn::visit::visit_impl_item_fn(scan, item));
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.body(|scan| syn::visit::visit_trait_item_fn(scan, item));
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        syn::visit::visit_expr_method_call(self, call);
        if call.method == "release" && call.args.is_empty() {
            self.released = true;
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        syn::visit::visit_expr_call(self, call);
        if self.released
            && let syn::Expr::Path(function) = &*call.func
            && self.names.claims(&function.path)
        {
            self.found.push(Finding {
                kind: Kind::ClaimAfterRelease,
                file: self.file.to_owned(),
                line: call.paren_token.span.open().start().line,
            });
        }
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        match invocation.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            Ok(expressions) => {
                for expression in &expressions {
                    self.visit_expr(expression);
                }
            }
            Err(_not_a_list_of_expressions) => {}
        }
        syn::visit::visit_macro(self, invocation);
    }
}
