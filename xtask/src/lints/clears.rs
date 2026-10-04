// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one door a test clears a nested process's environment through, which tells the process again to throw its coverage profile away.

use syn::visit::Visit;

use super::{Finding, Kind};

/// Why a test that clears an environment for itself is refused, in the words a person needs to fix it.
pub(super) const REMEDY: &str = "clear it through `njutest_devkit::paths::clear_environment`, \
    which tells the process again to throw its coverage profile away. A binary the coverage job \
    instrumented and told nothing about its profile writes `default_<signature>_<pid>.profraw` \
    into the directory it runs in, and a test's nested process runs in the tree under test: each \
    shard of a sharded run left one in the fixture, so the second shard measured a different \
    workspace from the first and the merge refused the two as two repositories. \
    `Command::env_clear` called, named as a path or handed to a macro is refused alike";

/// The method that clears a command's environment, its profile destination with it.
const CLEAR: &str = "env_clear";

/// Whether `file` is held to the rule, which is every file under a `tests` directory.
fn held(file: &str) -> bool {
    file.starts_with("tests/") || file.contains("/tests/")
}

/// Every place `parsed` clears a command's environment, each at its line, in code and among the tokens a macro is handed, where `file` is held to the rule.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    if !held(file) {
        return Vec::new();
    }
    let mut scan = Cleared {
        file,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Cleared<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

impl Cleared<'_> {
    fn note(&mut self, line: usize) {
        self.found.push(Finding {
            kind: Kind::RawEnvironmentClear,
            file: self.file.to_owned(),
            line,
        });
    }

    fn scan_tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        for tree in tokens.clone() {
            match tree {
                proc_macro2::TokenTree::Ident(ident) => {
                    if ident == CLEAR {
                        self.note(ident.span().start().line);
                    }
                }
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(&group.stream()),
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Cleared<'_> {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == CLEAR {
            self.note(call.method.span().start().line);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(last) = path.segments.last()
            && last.ident == CLEAR
        {
            self.note(last.ident.span().start().line);
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.scan_tokens(&invocation.tokens);
        syn::visit::visit_macro(self, invocation);
    }
}
