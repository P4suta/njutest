// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process-wide step checkpoints planted at runtime control-flow boundaries, and the entry markers planted beside them.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use syn::spanned::Spanned as _;
use syn::visit::{self, Visit};

use crate::span::Span;
use crate::splice::Splice;

use super::ItemBody;

/// Why checkpoints could not be placed without guessing at source offsets.
#[derive(Debug, thiserror::Error)]
pub(super) enum StepError {
    /// A byte-order mark or shebang prefix did not fit the source-span schema.
    #[error("the source prefix is out of range: {0}")]
    Prefix(#[from] crate::syntax::PrefixError),
    /// The source was not a Rust file.
    #[error("the source token stream is invalid: {0}")]
    Parse(#[from] syn::Error),
    /// A parser byte offset, an inline-module depth, or an item index did not fit the engine's representations.
    #[error("a checkpoint source offset, inline-module depth, or item index is out of range")]
    OutOfRange,
    /// Two syntactic boundaries at one byte would name different runtime modules.
    #[error("two checkpoints at byte {offset} require different runtime paths")]
    ConflictingPath { offset: u32 },
}

/// What one file is planted with: the insertions, and the items whose bodies the entry markers name.
pub(super) struct Planted {
    /// One-line insertions that charge the active mutation at function and loop boundaries and record each entered item.
    pub(super) splices: Vec<Splice>,
    /// Every item the file holds, in the order their indices were given from `first_item` up.
    pub(super) items: Vec<ItemBody>,
}

/// Plants the checkpoints and the entry markers of one file, numbering its items from `first_item`.
pub(super) fn plant(text: &str, module: &str, first_item: u32) -> Result<Planted, StepError> {
    let (base, parsed) = crate::syntax::strip_prefix(text)?;
    let file: syn::File = syn::parse_str(parsed)?;
    let mut collector = Collector {
        base,
        module,
        scope: crate::syntax::ModuleScope::root(),
        runtime: RuntimeContext::Allowed,
        insertions: BTreeMap::new(),
        error: None,
        names: Vec::new(),
        first_item,
        items: Vec::new(),
        entering: None,
    };
    collector.visit_file(&file);
    if let Some(error) = collector.error {
        return Err(error);
    }
    Ok(Planted {
        splices: collector
            .insertions
            .into_iter()
            .map(|(at, insertion)| Splice {
                span: Span { start: at, end: at },
                original: Vec::new(),
                replacement: insertion.render().into_bytes(),
            })
            .collect(),
        items: collector.items,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeContext {
    Allowed,
    Constant,
}

/// An item's name and the bytes it covers.
struct Named {
    name: String,
    span: Span,
}

struct Collector<'a> {
    base: u32,
    module: &'a str,
    scope: crate::syntax::ModuleScope,
    runtime: RuntimeContext,
    insertions: BTreeMap<u32, Insertion>,
    error: Option<StepError>,
    names: Vec<String>,
    first_item: u32,
    items: Vec<ItemBody>,
    entering: Option<u32>,
}

#[derive(Default)]
struct Insertion {
    closes: String,
    entry: Option<String>,
    checkpoint: Option<String>,
    opens: String,
}

impl Insertion {
    fn render(self) -> String {
        let Self {
            mut closes,
            entry,
            checkpoint,
            opens,
        } = self;
        if let Some(entry) = entry {
            closes.push_str(&entry);
        }
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

    fn named(&mut self, name: String, walk: impl FnOnce(&mut Self)) {
        self.names.push(name);
        walk(self);
        if self.names.pop().is_none() {
            self.error = Some(StepError::OutOfRange);
        }
    }

    fn entering_as(&mut self, entering: Option<u32>, walk: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.entering, entering);
        walk(self);
        self.entering = previous;
    }

    fn in_module(&mut self, items: &[syn::Item], walk: impl FnOnce(&mut Self)) {
        self.scope.enter(items);
        walk(self);
        if !self.scope.leave() {
            self.error = Some(StepError::OutOfRange);
        }
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

    fn span_of(&mut self, node: &impl syn::spanned::Spanned) -> Option<Span> {
        let range = node.span().byte_range();
        let start = self.absolute(range.start)?;
        let end = self.absolute(range.end)?;
        Some(Span { start, end })
    }

    fn called(&mut self, name: String, whole: &impl syn::spanned::Spanned) -> Option<Named> {
        let span = self.span_of(whole)?;
        Some(Named { name, span })
    }

    fn runtime_path(&mut self, function: &str) -> Option<String> {
        let Some(depth) = self.scope.supers() else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        Some(format!(
            "{}{}::{function}",
            "super::".repeat(depth),
            self.module
        ))
    }

    fn call(&mut self) -> Option<String> {
        Some(format!("{}(); ", self.runtime_path("checkpoint")?))
    }

    fn entry(&mut self) -> Option<String> {
        let index = self.entering?;
        Some(format!("{}({index}); ", self.runtime_path("item")?))
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
        let entry = self.entry();
        let insertion = self.insertions.entry(at).or_default();
        let conflicting = match &insertion.checkpoint {
            None => {
                insertion.checkpoint = Some(call);
                false
            }
            Some(existing) => existing != &call,
        };
        let conflicting = conflicting
            || match (&insertion.entry, entry) {
                (_, None) => false,
                (None, Some(entry)) => {
                    insertion.entry = Some(entry);
                    false
                }
                (Some(existing), Some(entry)) => existing != &entry,
            };
        if conflicting {
            self.error = Some(StepError::ConflictingPath { offset: at });
        }
    }

    fn checkpoint(&mut self, block: &syn::Block) {
        let relative = block.stmts.first().map_or_else(
            || block.brace_token.span.close().byte_range().start,
            |statement| statement.span().byte_range().start,
        );
        self.checkpoint_at(relative);
    }

    fn loop_checkpoint(&mut self, block: &syn::Block) {
        self.entering_as(None, |collector| collector.checkpoint(block));
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
        let entry = self.entry().unwrap_or_default();
        let written = write!(
            self.insertions.entry(start).or_default().opens,
            "{{ {entry}{call}"
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        self.insertions
            .entry(end)
            .or_default()
            .closes
            .push_str(" }");
    }

    fn item(&mut self, spans: (Span, Span), measurable: bool) -> Option<u32> {
        let Ok(ordinal) = u32::try_from(self.items.len()) else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        let Some(index) = self.first_item.checked_add(ordinal) else {
            self.error = Some(StepError::OutOfRange);
            return None;
        };
        if index == super::runtime::FIRST_UNREPRESENTABLE_INDEX {
            self.error = Some(StepError::OutOfRange);
            return None;
        }
        self.items.push(ItemBody {
            index,
            name: self.names.join("::"),
            span: spans.0,
            body: spans.1,
            measurable,
        });
        Some(index)
    }

    fn function(&mut self, named: Named, signature: &syn::Signature, block: &syn::Block) {
        let measurable = signature.constness.is_none();
        let context = if measurable {
            RuntimeContext::Allowed
        } else {
            RuntimeContext::Constant
        };
        let Named { name, span } = named;
        self.named(name, |collector| {
            let Some(body) = collector.span_of(block) else {
                return;
            };
            let index = collector.item((span, body), measurable);
            let entering = index.filter(|_| measurable);
            collector.in_context(context, |collector| {
                collector.entering_as(entering, |collector| {
                    collector.checkpoint(block);
                    visit::visit_block(collector, block);
                });
            });
        });
    }

    fn constant(&mut self, named: Named, value: &syn::Expr, walk: impl FnOnce(&mut Self)) {
        let Named { name, span } = named;
        self.named(name, |collector| {
            let Some(body) = collector.span_of(value) else {
                return;
            };
            if collector.item((span, body), false).is_none() {
                return;
            }
            collector.in_context(RuntimeContext::Constant, |collector| {
                collector.entering_as(None, walk);
            });
        });
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        visit::visit_signature(self, &node.sig);
        if let Some(named) = self.called(node.sig.ident.to_string(), node) {
            self.function(named, &node.sig, &node.block);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        visit::visit_signature(self, &node.sig);
        if let Some(named) = self.called(node.sig.ident.to_string(), node) {
            self.function(named, &node.sig, &node.block);
        }
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        visit::visit_signature(self, &node.sig);
        if let Some(block) = &node.default
            && let Some(named) = self.called(node.sig.ident.to_string(), node)
        {
            self.function(named, &node.sig, block);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.named(crate::syntax::implemented(node), |collector| {
            visit::visit_item_impl(collector, node);
        });
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.named(node.ident.to_string(), |collector| {
            visit::visit_item_trait(collector, node);
        });
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if let Some((_brace, items)) = &node.content {
            self.named(node.ident.to_string(), |collector| {
                collector.in_module(items, |collector| {
                    for item in items {
                        collector.visit_item(item);
                    }
                });
            });
        }
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let Some(named) = self.called(node.ident.to_string(), node) else {
            return;
        };
        self.constant(named, &node.expr, |collector| {
            visit::visit_item_const(collector, node);
        });
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let Some(named) = self.called(node.ident.to_string(), node) else {
            return;
        };
        self.constant(named, &node.expr, |collector| {
            visit::visit_item_static(collector, node);
        });
    }

    fn visit_impl_item_const(&mut self, node: &'ast syn::ImplItemConst) {
        let Some(named) = self.called(node.ident.to_string(), node) else {
            return;
        };
        self.constant(named, &node.expr, |collector| {
            visit::visit_impl_item_const(collector, node);
        });
    }

    fn visit_trait_item_const(&mut self, node: &'ast syn::TraitItemConst) {
        let Some((_, value)) = &node.default else {
            visit::visit_trait_item_const(self, node);
            return;
        };
        let Some(named) = self.called(node.ident.to_string(), node) else {
            return;
        };
        self.constant(named, value, |collector| {
            visit::visit_trait_item_const(collector, node);
        });
    }

    fn visit_expr_const(&mut self, node: &'ast syn::ExprConst) {
        self.in_context(RuntimeContext::Constant, |collector| {
            visit::visit_expr_const(collector, node);
        });
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.loop_checkpoint(&node.body);
        visit::visit_expr_loop(self, node);
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.loop_checkpoint(&node.body);
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.loop_checkpoint(&node.body);
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

    use super::{StepError, plant};
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
        let splices = plant(source, "__rm", 0)?.splices;
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
            planted.contains("|x| { __rm::item(0); __rm::checkpoint(); x }"),
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
            planted.contains(
                "|| { __rm::item(0); __rm::checkpoint(); || { __rm::item(0); __rm::checkpoint(); 1 } }"
            ),
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
            planted.contains(
                "fn f() { super::super::__rm::item(0); super::super::__rm::checkpoint();"
            ),
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
            planted.contains("fn runtime() { __rm::item(4); __rm::checkpoint();")
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
            planted.contains("fn main() { __rm::item(0); __rm::checkpoint();")
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
            planted.contains("#![allow(unused)] __rm::item(0); __rm::checkpoint(); loop"),
            "{planted}"
        );
        let parsed = syn::parse_file(&planted);
        assert_eq!(result_state(&parsed), Returned, "parse: {parsed:?}");
    }

    #[test]
    fn every_function_body_and_every_closure_in_it_names_the_item_it_is_in() {
        let planted = planted(
            "fn outer() { let _ = || { fn inner() {} }; let _ = async {}; } impl S { fn method(&self) -> u32 { 1 } }",
        );
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert!(
            planted.contains("fn outer() { __rm::item(0); __rm::checkpoint();")
                && planted.contains("|| { __rm::item(0); __rm::checkpoint(); fn inner() {__rm::item(1); __rm::checkpoint(); }")
                && planted.contains("async {__rm::item(0); __rm::checkpoint(); }")
                && planted.contains("fn method(&self) -> u32 { __rm::item(2); __rm::checkpoint(); 1 }"),
            "{planted}"
        );
        assert_eq!(
            planted.matches("__rm::item(").count(),
            5,
            "a loop is not an item, so it takes no entry marker: {planted}"
        );
    }

    #[test]
    fn a_loop_takes_a_checkpoint_and_no_entry_marker() {
        let planted = planted("fn f() { loop {} }");
        assert_eq!(result_state(&planted), Returned, "instrument: {planted:?}");
        let Ok(planted) = planted else { return };
        assert_eq!(planted.matches("__rm::item(").count(), 1, "{planted}");
        assert!(planted.contains("loop {__rm::checkpoint(); }"), "{planted}");
    }

    #[test]
    fn the_items_are_named_as_a_mutant_inside_them_names_its_item() {
        let source = "mod m { impl S { fn a() {} } impl T for S { fn b() {} } trait U { fn c() {} fn d(); const E: u8 = 1; } }";
        let planted = match plant(source, "__rm", 7) {
            Ok(planted) => planted,
            Err(error) => panic!("plant: {error}"),
        };
        let named: Vec<(u32, &str, bool)> = planted
            .items
            .iter()
            .map(|item| (item.index, item.name.as_str(), item.measurable))
            .collect();
        assert_eq!(
            named,
            [
                (7, "m::S::a", true),
                (8, "m::<S as T>::b", true),
                (9, "m::U::c", true),
                (10, "m::U::E", false),
            ],
            "a trait method without a body is no item, and a constant is one nothing can enter"
        );
    }

    #[test]
    fn a_const_fn_is_an_item_nothing_records_entering() {
        let source =
            "const fn c() -> u8 { 1 } static S: u8 = 2; fn f() { const fn g() {} fn h() {} }";
        let planted = match plant(source, "__rm", 0) {
            Ok(planted) => planted,
            Err(error) => panic!("plant: {error}"),
        };
        let named: Vec<(&str, bool)> = planted
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.measurable))
            .collect();
        assert_eq!(
            named,
            [
                ("c", false),
                ("S", false),
                ("f", true),
                ("f::g", false),
                ("f::h", true)
            ]
        );
        let text = planted_text(source);
        assert!(
            text.contains("const fn c() -> u8 { 1 }") && text.contains("const fn g() {}"),
            "{text}"
        );
    }

    #[test]
    fn an_item_spans_its_whole_text_and_its_body_its_braces() {
        let source = "/// Doc.\npub fn f(a: u8) -> u8 { a }";
        let planted = match plant(source, "__rm", 0) {
            Ok(planted) => planted,
            Err(error) => panic!("plant: {error}"),
        };
        let Some(item) = planted.items.first() else {
            panic!("one item");
        };
        let slice = |span: crate::span::Span| match (
            usize::try_from(span.start),
            usize::try_from(span.end),
        ) {
            (Ok(start), Ok(end)) => source.get(start..end),
            (Err(_), _) | (_, Err(_)) => None,
        };
        assert_eq!(slice(item.span), Some(source));
        assert_eq!(slice(item.body), Some("{ a }"));
    }

    #[test]
    fn a_loop_in_an_associated_constant_takes_no_runtime_call() {
        let text = planted_text(
            "impl S { const N: usize = { let mut n = 0; while n < 3 { n += 1; } n }; }",
        );
        assert!(!text.contains("__rm::"), "{text}");
    }

    fn planted_text(source: &str) -> String {
        match planted(source) {
            Ok(text) => text,
            Err(error) => panic!("{error}"),
        }
    }
}
