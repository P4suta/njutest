// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! OS buffers are owned and read through checked byte ranges, never through raw record pointers.

use syn::visit::Visit;

use super::{Finding, Kind};

pub(super) const REMEDY: &str = "use capdir::records::Buffer for aligned FFI storage and its lifetime-carrying Record reader; validate each record's size before reading its fields, and keep raw pointers only as arguments to the platform call";

const OWNER: &str = "crates/rust-mutants/src/capdir/records.rs";

pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    if file == OWNER {
        return Vec::new();
    }
    let mut scan = Pointers {
        file,
        unsafe_scope: false,
        opaque_argument: false,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Pointers<'a> {
    file: &'a str,
    unsafe_scope: bool,
    opaque_argument: bool,
    found: Vec<Finding>,
}

impl Pointers<'_> {
    fn note(&mut self, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind: Kind::RawBufferPointer,
            file: self.file.to_owned(),
            line: span.start().line,
        });
    }
}

fn memory_operation(name: &str) -> bool {
    matches!(
        name,
        "read_unaligned"
            | "read_volatile"
            | "write_unaligned"
            | "write_volatile"
            | "from_raw_parts"
            | "from_raw_parts_mut"
            | "copy_nonoverlapping"
            | "copy_from_nonoverlapping"
    )
}

fn unsafe_operation(name: &str) -> bool {
    memory_operation(name)
        || matches!(
            name,
            "read"
                | "write"
                | "add"
                | "sub"
                | "offset"
                | "byte_add"
                | "byte_sub"
                | "byte_offset"
                | "offset_from"
                | "offset_from_unsigned"
                | "byte_offset_from"
                | "byte_offset_from_unsigned"
                | "wrapping_add"
                | "wrapping_sub"
                | "wrapping_offset"
                | "as_ref"
                | "as_mut"
                | "copy"
                | "copy_to"
        )
}

fn wrapping_pointer(name: &str) -> bool {
    matches!(
        name,
        "wrapping_add"
            | "wrapping_sub"
            | "wrapping_offset"
            | "wrapping_byte_add"
            | "wrapping_byte_sub"
            | "wrapping_byte_offset"
    )
}

fn buffer_pointer(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(call) => {
            matches!(call.method.to_string().as_str(), "as_ptr" | "as_mut_ptr")
                || buffer_pointer(&call.receiver)
        }
        syn::Expr::Paren(paren) => buffer_pointer(&paren.expr),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Pointers<'_> {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        let previous = self.unsafe_scope;
        self.unsafe_scope = true;
        syn::visit::visit_expr_unsafe(self, node);
        self.unsafe_scope = previous;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let previous = self.unsafe_scope;
        if matches!(&node.sig.safety, syn::Safety::Unsafe(_)) {
            self.unsafe_scope = true;
        }
        syn::visit::visit_item_fn(self, node);
        self.unsafe_scope = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let previous = self.unsafe_scope;
        if matches!(&node.sig.safety, syn::Safety::Unsafe(_)) {
            self.unsafe_scope = true;
        }
        syn::visit::visit_impl_item_fn(self, node);
        self.unsafe_scope = previous;
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref()
            && let Some(name) = path.path.segments.last()
            && (memory_operation(&name.ident.to_string())
                || (self.unsafe_scope && unsafe_operation(&name.ident.to_string())))
        {
            self.note(name.ident.span());
        }
        self.visit_expr(&node.func);
        for argument in &node.args {
            let previous = self.opaque_argument;
            self.opaque_argument = self.unsafe_scope;
            self.visit_expr(argument);
            self.opaque_argument = previous;
        }
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.imports(&node.tree, false);
        syn::visit::visit_item_use(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if memory_operation(&name)
            || wrapping_pointer(&name)
            || (self.unsafe_scope && unsafe_operation(&name))
            || (matches!(name.as_str(), "cast" | "cast_mut" | "cast_const")
                && (node.turbofish.is_some()
                    || (buffer_pointer(&node.receiver) && !self.opaque_argument)))
        {
            self.note(node.method.span());
        }
        let previous = self.opaque_argument;
        self.opaque_argument = false;
        syn::visit::visit_expr_method_call(self, node);
        self.opaque_argument = previous;
    }

    fn visit_expr_unary(&mut self, node: &'ast syn::ExprUnary) {
        if self.unsafe_scope
            && let syn::UnOp::Deref(star) = node.op
        {
            self.note(star.span);
        }
        syn::visit::visit_expr_unary(self, node);
    }

    fn visit_ident(&mut self, node: &'ast proc_macro2::Ident) {
        if memory_operation(&node.to_string()) {
            self.note(node.span());
        }
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.tokens(&node.tokens, self.unsafe_scope);
    }
}

impl Pointers<'_> {
    fn imports(&mut self, tree: &syn::UseTree, pointer_module: bool) {
        match tree {
            syn::UseTree::Path(path) => {
                self.imports(&path.tree, pointer_module || path.ident == "ptr");
            }
            syn::UseTree::Name(name)
                if pointer_module && unsafe_operation(&name.ident.to_string()) =>
            {
                self.note(name.ident.span());
            }
            syn::UseTree::Rename(name)
                if pointer_module && unsafe_operation(&name.ident.to_string()) =>
            {
                self.note(name.ident.span());
            }
            syn::UseTree::Group(group) => {
                for child in &group.items {
                    self.imports(child, pointer_module);
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
        }
    }

    fn tokens(&mut self, tokens: &proc_macro2::TokenStream, unsafe_scope: bool) {
        let unsafe_scope = unsafe_scope || super::macro_tokens_name(tokens, "unsafe");
        let buffer_cast = (super::macro_tokens_name(tokens, "as_ptr")
            || super::macro_tokens_name(tokens, "as_mut_ptr"))
            && super::macro_tokens_name(tokens, "cast");
        for token in tokens.clone() {
            match token {
                proc_macro2::TokenTree::Ident(name)
                    if memory_operation(&name.to_string())
                        || wrapping_pointer(&name.to_string())
                        || (unsafe_scope && unsafe_operation(&name.to_string()))
                        || (buffer_cast && name == "cast") =>
                {
                    self.note(name.span());
                }
                proc_macro2::TokenTree::Group(group) => self.tokens(&group.stream(), unsafe_scope),
                proc_macro2::TokenTree::Punct(star) if unsafe_scope && star.as_char() == '*' => {
                    self.note(star.span());
                }
                proc_macro2::TokenTree::Ident(_)
                | proc_macro2::TokenTree::Literal(_)
                | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}
