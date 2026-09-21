// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process-wide step checkpoints planted at runtime control-flow boundaries.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use syn::spanned::Spanned as _;
use syn::visit::{self, Visit};

use crate::span::Span;
use crate::splice::Splice;

/// Why checkpoints could not be placed without guessing at source offsets.
#[derive(Debug, thiserror::Error)]
pub(super) enum StepError {
    /// A byte-order mark or shebang prefix did not fit the source-span schema.
    #[error("the source prefix is out of range: {0}")]
    Prefix(#[from] crate::syntax::PrefixError),
    /// The source was not a Rust file.
    #[error("the source token stream is invalid: {0}")]
    Parse(#[from] syn::Error),
    /// A parser byte offset or inline-module depth did not fit the engine's representations.
    #[error("a checkpoint source offset or inline-module depth is out of range")]
    OutOfRange,
    /// Two syntactic boundaries at one byte would name different runtime modules.
    #[error("two checkpoints at byte {offset} require different runtime paths")]
    ConflictingPath { offset: u32 },
}

/// One-line insertions that charge the active mutation at function and loop boundaries.
pub(super) fn splices(text: &str, module: &str) -> Result<Vec<Splice>, StepError> {
    let (base, parsed) = crate::syntax::strip_prefix(text)?;
    let file: syn::File = syn::parse_str(parsed)?;
    let mut collector = Collector {
        base,
        module,
        module_depth: 0,
        runtime: RuntimeContext::Allowed,
        insertions: BTreeMap::new(),
        error: None,
    };
    collector.visit_file(&file);
    if let Some(error) = collector.error {
        return Err(error);
    }
    Ok(collector
        .insertions
        .into_iter()
        .map(|(at, insertion)| Splice {
            span: Span { start: at, end: at },
            original: Vec::new(),
            replacement: insertion.render().into_bytes(),
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeContext {
    Allowed,
    Constant,
}

struct Collector<'a> {
    base: u32,
    module: &'a str,
    module_depth: u32,
    runtime: RuntimeContext,
    insertions: BTreeMap<u32, Insertion>,
    error: Option<StepError>,
}

#[derive(Default)]
struct Insertion {
    closes: String,
    checkpoint: Option<String>,
    opens: String,
}

impl Insertion {
    fn render(self) -> String {
        let Self {
            mut closes,
            checkpoint,
            opens,
        } = self;
        if let Some(checkpoint) = checkpoint {
            closes.push_str(&checkpoint);
        }
        closes.push_str(&opens);
        closes
    }
}

impl Collector<'_> {
    fn in_context(&mut self, context: RuntimeContext, walk: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.runtime, context);
        walk(self);
        self.runtime = previous;
    }

    fn in_module(&mut self, walk: impl FnOnce(&mut Self)) {
        let Some(depth) = self.module_depth.checked_add(1) else {
            self.error = Some(StepError::OutOfRange);
            return;
        };
        self.module_depth = depth;
        walk(self);
        let Some(depth) = self.module_depth.checked_sub(1) else {
            self.error = Some(StepError::OutOfRange);
            return;
        };
        self.module_depth = depth;
    }

    fn absolute(&mut self, relative: usize) -> Option<u32> {
        let Ok(relative) = u32::try_from(relative) else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        let Some(absolute) = self.base.checked_add(relative) else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        Some(absolute)
    }

    fn call(&mut self) -> Option<String> {
        let Ok(depth) = usize::try_from(self.module_depth) else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        Some(format!(
            "{}{}::checkpoint(); ",
            "super::".repeat(depth),
            self.module
        ))
    }

    fn checkpoint_at(&mut self, relative: usize) {
        if self.runtime == RuntimeContext::Constant || self.error.is_some() {
            return;
        }
        let Some(at) = self.absolute(relative) else {
            return;
        };
        let Some(call) = self.call() else {
            return;
        };
        let insertion = self.insertions.entry(at).or_default();
        match &insertion.checkpoint {
            None => insertion.checkpoint = Some(call),
            Some(existing) if existing == &call => {}
            Some(_) => self.error = Some(StepError::ConflictingPath { offset: at }),
        }
    }

    fn checkpoint(&mut self, block: &syn::Block) {
        let relative = block.stmts.first().map_or_else(
            || block.brace_token.span.close().byte_range().start,
            |statement| statement.span().byte_range().start,
        );
        self.checkpoint_at(relative);
    }

    fn expression_closure(&mut self, body: &syn::Expr) {
        if self.runtime == RuntimeContext::Constant || self.error.is_some() {
            return;
        }
        let range = body.span().byte_range();
        let Some(start) = self.absolute(range.start) else {
            return;
        };
        let Some(end) = self.absolute(range.end) else {
            return;
        };
        let Some(call) = self.call() else {
            return;
        };
        let written = write!(self.insertions.entry(start).or_default().opens, "{{ {call}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        self.insertions
            .entry(end)
            .or_default()
            .closes
            .push_str(" }");
    }

    fn function(&mut self, signature: &syn::Signature, block: &syn::Block) {
        let context = if signature.constness.is_some() {
            RuntimeContext::Constant
        } else {
            RuntimeContext::Allowed
        };
        self.in_context(context, |collector| {
            collector.checkpoint(block);
            visit::visit_block(collector, block);
        });
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        visit::visit_signature(self, &node.sig);
        self.function(&node.sig, &node.block);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        visit::visit_signature(self, &node.sig);
        self.function(&node.sig, &node.block);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        visit::visit_signature(self, &node.sig);
        if let Some(block) = &node.default {
            self.function(&node.sig, block);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if let Some((_brace, items)) = &node.content {
            self.in_module(|collector| {
                for item in items {
                    collector.visit_item(item);
                }
            });
        }
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.in_context(RuntimeContext::Constant, |collector| {
            visit::visit_item_const(collector, node);
        });
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.in_context(RuntimeContext::Constant, |collector| {
            visit::visit_item_static(collector, node);
        });
    }

    fn visit_expr_const(&mut self, node: &'ast syn::ExprConst) {
        self.in_context(RuntimeContext::Constant, |collector| {
            visit::visit_expr_const(collector, node);
        });
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.checkpoint(&node.body);
        visit::visit_expr_loop(self, node);
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.checkpoint(&node.body);
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.checkpoint(&node.body);
        visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        self.checkpoint(&node.block);
        visit::visit_expr_async(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        if node.constness.is_some() {
            self.in_context(RuntimeContext::Constant, |collector| {
                visit::visit_expr_closure(collector, node);
            });
            return;
        }
        self.in_context(RuntimeContext::Allowed, |collector| {
            match &*node.body {
                syn::Expr::Block(block) => collector.checkpoint(&block.block),
                expression => collector.expression_closure(expression),
            }
            visit::visit_expr_closure(collector, node);
        });
    }
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState::Returned, result_state};

    use super::{StepError, splices};
    use crate::splice::apply;

    #[derive(Debug, thiserror::Error)]
    enum PlantError {
        #[error(transparent)]
        Step(#[from] StepError),
        #[error(transparent)]
        Splice(#[from] crate::splice::SpliceError),
        #[error(transparent)]
        Utf8(#[from] std::string::FromUtf8Error),
        #[error("the splice map recorded {actual} checkpoints for {expected} insertions")]
        OffsetCount { actual: usize, expected: usize },
    }

    fn planted(source: &str) -> Result<String, PlantError> {
        let splices = splices(source, "__rm")?;
        let (planted, offsets) = apply(source.as_bytes(), &splices)?;
        if offsets.splices() != splices.len() {
            return Err(PlantError::OffsetCount {
                actual: offsets.splices(),
                expected: splices.len(),
            });
        }
        Ok(String::from_utf8(planted)?)
    }

    #[test]
    fn functions_loops_async_blocks_and_both_closure_forms_receive_checkpoints() {
        let source = "fn f() { loop {} while true {} for _ in 0..1 {} let _ = || {}; let _ = |x| x; let _ = async {}; }";
        let planted = planted(source);
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert_eq!(planted.matches("__rm::checkpoint();").count(), 7);
        assert!(
            planted.contains("|x| { __rm::checkpoint(); x }"),
            "{planted}"
        );
        let parsed = syn::parse_file(&planted);
        assert_eq!(result_state(&parsed), Returned, "parse: {parsed:?}");
    }

    #[test]
    fn nested_expression_closures_close_in_source_order() {
        let planted = planted("fn f() { let _ = || || 1; }");
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert_eq!(planted.matches("__rm::checkpoint();").count(), 3);
        assert!(
            planted.contains("|| { __rm::checkpoint(); || { __rm::checkpoint(); 1 } }"),
            "{planted}"
        );
        let parsed = syn::parse_file(&planted);
        assert_eq!(result_state(&parsed), Returned, "parse: {parsed:?}");
    }

    #[test]
    fn inline_modules_name_the_file_root_runtime() {
        let planted = planted("mod one { mod two { fn f() { loop {} } } }");
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert!(
            planted.contains("fn f() { super::super::__rm::checkpoint();"),
            "{planted}"
        );
        assert_eq!(
            planted.matches("super::super::__rm::checkpoint();").count(),
            2
        );
    }

    #[test]
    fn constant_contexts_receive_no_runtime_calls_but_nested_runtime_functions_do() {
        let planted = planted(
            "const X: usize = { loop { break 1 } }; const fn c() { loop {} } const F: fn() = || loop {}; const _: () = { fn runtime() { loop {} } };",
        );
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert_eq!(planted.matches("__rm::checkpoint();").count(), 4);
        assert!(
            planted.contains("fn runtime() { __rm::checkpoint();")
                && planted.contains("|| { __rm::checkpoint(); loop {__rm::checkpoint();"),
            "{planted}"
        );
    }

    #[test]
    fn a_shebang_and_bom_do_not_move_parser_offsets() {
        let planted = planted("\u{feff}#!/usr/bin/env rustx\nfn main() { loop {} }");
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert!(
            planted.contains("fn main() { __rm::checkpoint();")
                && planted.contains("loop {__rm::checkpoint();"),
            "{planted}"
        );
    }

    #[test]
    fn a_checkpoint_follows_a_block_inner_attribute() {
        let planted = planted("fn f() { #![allow(unused)] loop {} }");
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert!(
            planted.contains("#![allow(unused)] __rm::checkpoint(); loop"),
            "{planted}"
        );
        let parsed = syn::parse_file(&planted);
        assert_eq!(result_state(&parsed), Returned, "parse: {parsed:?}");
    }
}
