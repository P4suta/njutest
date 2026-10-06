// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Credential-shaped names must carry the redacting representation instead of inventing a secret source.

use syn::visit::Visit;

use super::{Finding, Kind};

pub(super) const REMEDY: &str = "name non-sensitive data for its domain (harness_report, counts, unreadable_file); actual credentials use rust_mutants::sensitive::Sensitive<T>, whose Debug and Display redact, and expose them only to the operation that needs them";

pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut scan = Names {
        file,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Names<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let possible_identity = name.contains("account")
        && !["accounted", "accounting", "accountable", "accountant"]
            .iter()
            .any(|word| name.contains(word));
    possible_identity
        || [
            "password",
            "passwd",
            "secret",
            "credential",
            "api_key",
            "apikey",
            "access_token",
            "auth_token",
            "private_key",
        ]
        .iter()
        .any(|word| name.contains(word))
}

fn redacted(held: &syn::Type) -> bool {
    let syn::Type::Path(held) = held else {
        return false;
    };
    let names: Vec<String> = held
        .path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
    matches!(names.as_slice(), [root, module, item] if (root == "rust_mutants" || root == "crate") && module == "sensitive" && item == "Sensitive")
}

impl Names<'_> {
    fn note(&mut self, name: &proc_macro2::Ident, held: Option<&syn::Type>) {
        if sensitive(&name.to_string()) && !held.is_some_and(redacted) {
            self.found.push(Finding {
                kind: Kind::SensitiveName,
                file: self.file.to_owned(),
                line: name.span().start().line,
            });
        }
    }

    fn binding(&mut self, pattern: &syn::Pat, held: Option<&syn::Type>) {
        match pattern {
            syn::Pat::Ident(name) => self.note(&name.ident, held),
            syn::Pat::Type(typed) => self.binding(&typed.pat, Some(&typed.ty)),
            syn::Pat::Tuple(tuple) => {
                for pattern in &tuple.elems {
                    self.binding(pattern, None);
                }
            }
            syn::Pat::Struct(fields) => {
                for field in &fields.fields {
                    self.binding(&field.pat, None);
                }
            }
            syn::Pat::TupleStruct(tuple) => {
                for pattern in &tuple.elems {
                    self.binding(pattern, None);
                }
            }
            syn::Pat::Slice(slice) => {
                for pattern in &slice.elems {
                    self.binding(pattern, None);
                }
            }
            syn::Pat::Or(alternatives) => {
                for pattern in &alternatives.cases {
                    self.binding(pattern, None);
                }
            }
            syn::Pat::Reference(reference) => self.binding(&reference.pat, held),
            syn::Pat::Paren(paren) => self.binding(&paren.pat, held),
            _ => {}
        }
    }

    fn tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        for token in tokens.clone() {
            match token {
                proc_macro2::TokenTree::Ident(name) => self.note(&name, None),
                proc_macro2::TokenTree::Group(group) => self.tokens(&group.stream()),
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Names<'_> {
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.note(&node.ident, Some(&node.ty));
        syn::visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.note(&node.ident, Some(&node.ty));
        syn::visit::visit_item_static(self, node);
    }

    fn visit_impl_item_const(&mut self, node: &'ast syn::ImplItemConst) {
        self.note(&node.ident, Some(&node.ty));
        syn::visit::visit_impl_item_const(self, node);
    }

    fn visit_trait_item_const(&mut self, node: &'ast syn::TraitItemConst) {
        self.note(&node.ident, Some(&node.ty));
        syn::visit::visit_trait_item_const(self, node);
    }

    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        self.note(&node.ident, None);
        syn::visit::visit_variant(self, node);
    }

    fn visit_signature(&mut self, node: &'ast syn::Signature) {
        let output = match &node.output {
            syn::ReturnType::Type(_, held) => Some(held.as_ref()),
            syn::ReturnType::Default => None,
        };
        if let Some(held) = output {
            self.note(&node.ident, Some(held));
        }
        syn::visit::visit_signature(self, node);
    }

    fn visit_field(&mut self, node: &'ast syn::Field) {
        if let Some(name) = &node.ident {
            self.note(name, Some(&node.ty));
        }
        syn::visit::visit_field(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        self.binding(&node.pat, None);
        syn::visit::visit_local(self, node);
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.binding(&node.pat, None);
        syn::visit::visit_arm(self, node);
    }

    fn visit_fn_arg(&mut self, node: &'ast syn::FnArg) {
        if let syn::FnArg::Typed(argument) = node {
            self.binding(&argument.pat, Some(&argument.ty));
        }
        syn::visit::visit_fn_arg(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        for pattern in &node.inputs {
            self.binding(pattern, None);
        }
        syn::visit::visit_expr_closure(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.binding(&node.pat, None);
        syn::visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
        self.binding(&node.pat, None);
        syn::visit::visit_expr_let(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if node.path.is_ident("macro_rules") {
            self.tokens(&node.tokens);
        }
        syn::visit::visit_macro(self, node);
    }
}
