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
        found: Vec::new(),
    };
    scan.visit_file(source);
    scan.found
}

struct Scan<'a> {
    file: &'a str,
    temporary: BTreeSet<String>,
    found: Vec<Finding>,
}

impl<'ast> syn::visit::Visit<'ast> for Scan<'_> {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let outer = self.temporary.clone();
        syn::visit::visit_block(self, block);
        self.temporary = outer;
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let syn::Pat::Ident(name) = &local.pat {
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
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = call.func.as_ref()
            && function.path.segments.last().is_some_and(|name| {
                name.ident == "cached"
                    || name.ident == "configured"
                    || name.ident == "with_compiler"
                    || (name.ident == "retained"
                        && function
                            .path
                            .segments
                            .iter()
                            .any(|part| part.ident == "CompilationCache"))
            })
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
