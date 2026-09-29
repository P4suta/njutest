// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The files that start git, each asking it to read the tree itself rather than a file-system monitor or a cache that answers for it.

use syn::visit::Visit;

use super::{Finding, Kind};

/// Every file that starts git: the engine's one asker, the tests' repository helper and xtask's, which say `core.fsmonitor=false` and `core.untrackedCache=false`, and the doctor, which asks git only its version.
pub(super) const DOORS: [&str; 4] = [
    "crates/rust-mutants/src/git.rs",
    "crates/njutest-devkit/src/repo.rs",
    "xtask/src/repository.rs",
    "crates/rust-mutants-cli/src/app/doctor.rs",
];

/// Whether `file` is held to the rule, which is every file the gate reads but the doors.
pub(super) fn held(file: &str) -> bool {
    !DOORS.contains(&file)
}

/// Every place `parsed` names git as the program it starts, each at its line.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut scan = Started {
        file,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Started<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

/// Whether `expression` is the string literal `git`.
fn names_git(expression: &syn::Expr) -> bool {
    matches!(
        expression,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) if text.value() == "git"
    )
}

/// The owner and the name of the function `path` calls, however it is qualified.
fn called(path: &syn::Path) -> Option<(String, String)> {
    let mut segments = path.segments.iter().rev();
    let last = segments.next()?;
    let owner = segments.next()?;
    Some((owner.ident.to_string(), last.ident.to_string()))
}

/// Whether `path` is a constructor a program's name is given to: `Command::new`, `OsString::from` or `OsStr::new`.
fn starts_a_program(path: &syn::Path) -> bool {
    called(path).is_some_and(|(owner, name)| {
        matches!(
            (owner.as_str(), name.as_str()),
            ("Command" | "OsStr", "new") | ("OsString", "from")
        )
    })
}

/// Whether `path` is the constructor an argument vector is given to: `Spec::new`.
fn takes_an_argv(path: &syn::Path) -> bool {
    called(path).is_some_and(|(owner, name)| owner == "Spec" && name == "new")
}

/// Whether `expression` is an argument vector whose program is git: an array or a `vec!` whose first element is `git`.
fn argv_of_git(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Array(array) => array.elems.first().is_some_and(names_git),
        syn::Expr::Macro(invoked) if invoked.mac.path.is_ident("vec") => {
            match listed(&invoked.mac.tokens) {
                Ok(elements) => elements.first().is_some_and(names_git),
                Err(_not_expressions) => false,
            }
        }
        syn::Expr::Reference(borrowed) => argv_of_git(&borrowed.expr),
        _ => false,
    }
}

/// The expressions a `vec!` lists, where its tokens are a list of expressions.
fn listed(
    tokens: &proc_macro2::TokenStream,
) -> syn::Result<syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>> {
    syn::parse::Parser::parse2(
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        tokens.clone(),
    )
}

impl Started<'_> {
    fn note(&mut self, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind: Kind::RawGit,
            file: self.file.to_owned(),
            line: span.start().line,
        });
    }
}

impl<'ast> Visit<'ast> for Started<'_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*node.func
            && ((starts_a_program(&function.path) && node.args.first().is_some_and(names_git))
                || (takes_an_argv(&function.path) && node.args.first().is_some_and(argv_of_git)))
        {
            self.note(node.paren_token.span.open());
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if node.path.is_ident("vec") {
            match listed(&node.tokens) {
                Ok(elements) => {
                    for element in &elements {
                        self.visit_expr(element);
                    }
                }
                Err(_not_expressions) => {}
            }
        }
        syn::visit::visit_macro(self, node);
    }
}
