// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Temporary cleanup cannot own a compilation cache's asynchronous producers.

use std::collections::BTreeSet;

use syn::visit::Visit as _;

use super::{Finding, Kind};

/// Raw temporary paths passed to a compilation-cache consumer.
pub(super) fn found(source: &syn::File, file: &str) -> Vec<Finding> {
    let mut scan = Scan {
        file,
        temporary: BTreeSet::new(),
        bindings: Bindings::new(),
        found: Vec::new(),
    };
    scan.visit_file(source);
    scan.found
}

struct Scan<'a> {
    file: &'a str,
    temporary: BTreeSet<String>,
    bindings: Bindings,
    found: Vec<Finding>,
}

/// Cache types and constructor values visible in one lexical scope.
#[derive(Clone)]
struct Bindings {
    runners: BTreeSet<String>,
    caches: BTreeSet<String>,
    constructors: BTreeSet<String>,
}

impl Bindings {
    fn new() -> Self {
        Self {
            runners: BTreeSet::from(["SealedRunner".to_owned()]),
            caches: BTreeSet::from(["CompilationCache".to_owned()]),
            constructors: BTreeSet::new(),
        }
    }

    fn constructor(&self, expression: &syn::Expr) -> bool {
        match expression {
            syn::Expr::Path(function) => {
                let names: Vec<_> = function
                    .path
                    .segments
                    .iter()
                    .map(|part| part.ident.to_string())
                    .collect();
                match names.as_slice() {
                    [name] => self.constructors.contains(name),
                    [.., owner, method] => {
                        (self.runners.contains(owner)
                            && matches!(method.as_str(), "cached" | "configured" | "with_compiler"))
                            || (self.caches.contains(owner) && method == "retained")
                    }
                    [] => false,
                }
            }
            syn::Expr::Paren(group) => self.constructor(&group.expr),
            syn::Expr::Group(group) => self.constructor(&group.expr),
            _ => false,
        }
    }

    fn imports(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => self.imports(&path.tree),
            syn::UseTree::Name(name) => {
                self.alias(&name.ident.to_string(), &name.ident.to_string());
            }
            syn::UseTree::Rename(name) => {
                self.alias(&name.ident.to_string(), &name.rename.to_string());
            }
            syn::UseTree::Group(group) => {
                for child in &group.items {
                    self.imports(child);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    fn alias(&mut self, original: &str, alias: &str) {
        if self.runners.contains(original) {
            self.runners.insert(alias.to_owned());
        } else if self.caches.contains(original) {
            self.caches.insert(alias.to_owned());
        }
    }

    fn items(&mut self, items: &[syn::Item]) {
        for item in items {
            if let syn::Item::Use(import) = item {
                self.imports(&import.tree);
            }
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for Scan<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.bindings.items(&file.items);
        syn::visit::visit_file(self, file);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let outer = self.bindings.clone();
        if let Some((_, items)) = &module.content {
            self.bindings.items(items);
        }
        syn::visit::visit_item_mod(self, module);
        self.bindings = outer;
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let outer = self.temporary.clone();
        let bindings = self.bindings.clone();
        for statement in &block.stmts {
            if let syn::Stmt::Item(syn::Item::Use(import)) = statement {
                self.bindings.imports(&import.tree);
            }
        }
        syn::visit::visit_block(self, block);
        self.temporary = outer;
        self.bindings = bindings;
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        syn::visit::visit_local(self, local);
        if let syn::Pat::Ident(name) = &local.pat {
            let constructor = local
                .init
                .as_ref()
                .is_some_and(|init| self.bindings.constructor(&init.expr));
            if constructor {
                self.bindings.constructors.insert(name.ident.to_string());
            } else {
                self.bindings.constructors.remove(&name.ident.to_string());
            }
            let raw = local
                .init
                .as_ref()
                .is_some_and(|init| tainted(&init.expr, &self.temporary));
            if raw {
                self.temporary.insert(name.ident.to_string());
            } else {
                self.temporary.remove(&name.ident.to_string());
            }
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.bindings.constructor(&call.func)
            && call
                .args
                .last()
                .is_some_and(|argument| tainted(argument, &self.temporary))
        {
            self.found.push(Finding {
                kind: Kind::UnownedCacheDirectory,
                file: self.file.to_owned(),
                line: call.paren_token.span.open().start().line,
            });
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// Whether an expression contains a path whose cleanup belongs to a child `TempDir`.
fn tainted(expression: &syn::Expr, temporary: &BTreeSet<String>) -> bool {
    struct Taint<'a> {
        temporary: &'a BTreeSet<String>,
        found: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for Taint<'_> {
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            if path.path.segments.iter().any(|name| {
                self.temporary.contains(&name.ident.to_string())
                    || name.ident == "tempdir"
                    || name.ident == "TempDir"
                    || name.ident == "Temporary"
            }) {
                self.found = true;
            }
            syn::visit::visit_expr_path(self, path);
        }

        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if call.method == "tempdir" || call.method == "tempdir_in" {
                self.found = true;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }
    let mut scan = Taint {
        temporary,
        found: false,
    };
    scan.visit_expr(expression);
    scan.found
}
