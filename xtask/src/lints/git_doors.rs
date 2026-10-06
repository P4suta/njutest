// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The files that start git, each asking it to read the tree itself rather than a file-system monitor or a cache that answers for it.

use std::collections::BTreeSet;

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

/// Every place `parsed` names git as the program it starts, each at its line: directly, or handed to a function or method of the file that starts the program one of its parameters names.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut helpers = Helpers::default();
    helpers.visit_file(parsed);
    let mut scan = Started {
        file,
        helpers: &helpers.found,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Started<'a> {
    file: &'a str,
    helpers: &'a BTreeSet<String>,
    found: Vec<Finding>,
}

/// The functions and methods of a file that start the program a parameter of theirs names, found before any call is read.
#[derive(Default)]
struct Helpers {
    within: Vec<(String, BTreeSet<String>)>,
    found: BTreeSet<String>,
}

impl Helpers {
    fn enter(&mut self, signature: &syn::Signature) {
        let parameters = signature
            .inputs
            .iter()
            .filter_map(|input| match input {
                syn::FnArg::Typed(typed) => match &*typed.pat {
                    syn::Pat::Ident(named) => Some(named.ident.to_string()),
                    _ => None,
                },
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        self.within.push((signature.ident.to_string(), parameters));
    }
}

/// Whether `expression` is a bare name among `parameters`.
fn one_of(expression: &syn::Expr, parameters: &BTreeSet<String>) -> bool {
    match expression {
        syn::Expr::Path(named) => named
            .path
            .get_ident()
            .is_some_and(|ident| parameters.contains(&ident.to_string())),
        syn::Expr::Reference(borrowed) => one_of(&borrowed.expr, parameters),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Helpers {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.enter(&node.sig);
        syn::visit::visit_item_fn(self, node);
        self.within.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.enter(&node.sig);
        syn::visit::visit_impl_item_fn(self, node);
        self.within.pop();
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*node.func
            && starts_a_program(&function.path)
            && let Some((name, parameters)) = self.within.last()
            && node
                .args
                .first()
                .is_some_and(|program| one_of(program, parameters))
        {
            self.found.insert(name.clone());
        }
        syn::visit::visit_expr_call(self, node);
    }
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
                || (takes_an_argv(&function.path) && node.args.first().is_some_and(argv_of_git))
                || (function
                    .path
                    .segments
                    .last()
                    .is_some_and(|last| self.helpers.contains(&last.ident.to_string()))
                    && node.args.iter().any(names_git)))
        {
            self.note(node.paren_token.span.open());
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if self.helpers.contains(&node.method.to_string()) && node.args.iter().any(names_git) {
            self.note(node.paren_token.span.open());
        }
        syn::visit::visit_expr_method_call(self, node);
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
