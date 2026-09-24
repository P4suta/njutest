// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The walk over one file's syntax tree.

use std::cell::Cell;
use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::{
    Attribute, BinOp, Block, Expr, ImplItem, Item, Macro, Pat, ReturnType, Signature, Stmt,
    TraitItem,
};

use super::annotate::Marker;
use super::branch;
use super::position::LineIndex;
use super::rules::{
    arguments, assertion_arity, assertion_is_condition, binary_swap, bool_method, has_let,
    is_bool_literal, is_compound_assignment, is_connective, is_default_spelling, is_err_default,
    is_not, is_ok_default, is_some_default, is_true_literal, method_swap, respell_int,
    terminal_else, unary_removal,
};
use super::shape::{
    block_tail, deletable_arm, expr_attrs, guard_of, implemented, item_attrs, parameters,
    return_kind, return_kind_within, suppression_of,
};
use super::{Claim, Decision, Form, Found, Include, Selection, SiteHint, SkipReason, beside};
use crate::catalog::Candidate;
use crate::probe::Question;
use crate::span::Span;

/// What the enclosing function returns, as far as its signature says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReturnKind {
    /// A closure without a spelled type, an async block: nothing is known.
    Unknown,
    /// `()` or no return type.
    Unit,
    /// `-> !`.
    Never,
    /// `-> bool`.
    Bool,
    /// `-> Result<..>` by its last path segment, and whether the syntax can say each of the two types it names has a default.
    Result {
        /// Whether the `Ok` type spells a default.
        ok: bool,
        /// Whether the `Err` type spells a default.
        err: bool,
    },
    /// `-> Option<..>` by its last path segment, and whether the syntax can say the `Some` type has a default.
    Option(bool),
    /// Anything else, where only `Default::default()` can be offered.
    Other,
    /// A type the syntax cannot say has a default, so the replacement is stated rather than guessed.
    Unstated,
}

/// Where a statement without a semicolon sits in its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TailRole {
    /// Not the last statement: a block-like statement whose value is `()`.
    NotLast,
    /// The last statement: the block's value.
    BlockValue,
    /// The last statement of a function body: what the function returns.
    ReturnValue,
}

/// The innermost function-like scope.
#[derive(Debug, Clone, Copy)]
struct Frame {
    ret: ReturnKind,
}

/// A guard site.
#[derive(Debug, Clone, Copy)]
struct Site {
    form: Form,
    span: Span,
}

/// Everything an offered return replacement inherits from its expression.
#[derive(Debug, Clone, Copy)]
struct ReturnOffer {
    span: Span,
    site: Option<Site>,
    probeable: bool,
}

/// Which `Result` arms have a return type whose spelling promises `Default`.
#[derive(Debug, Clone, Copy)]
struct ResultDefaults {
    ok: bool,
    err: bool,
}

/// What an expression's position allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Syntactically `bool`: Form C.
    Bool,
    /// Any value: Form E.
    Value,
    /// A place that must stay a place: no form.
    Place,
}

/// Where one decision was made, and what the walker has to say about it.
#[derive(Debug, Clone, Copy)]
struct At<'a> {
    offset: u32,
    rule: &'a str,
    note: Option<&'a str>,
}

impl<'a> At<'a> {
    const fn new(offset: u32, rule: &'a str) -> Self {
        Self {
            offset,
            rule,
            note: None,
        }
    }

    const fn noting(mut self, note: &'a str) -> Self {
        self.note = Some(note);
        self
    }
}

/// The context an expression is walked in.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    kind: Kind,
    /// The statement-level site, for an edit that changes its expression's type.
    stmt: Option<Site>,
    /// Whether the expression is the whole of an expression statement.
    direct_stmt: bool,
    /// Whether another rule already offers to negate this expression whole.
    negated: bool,
    /// Where a swap that changes the expression's type has to be guarded: the end of the method chain this call is a receiver in.
    wrap: Option<Span>,
}

impl Ctx {
    const fn new(kind: Kind, stmt: Option<Site>) -> Self {
        Self {
            kind,
            stmt,
            direct_stmt: false,
            negated: false,
            wrap: None,
        }
    }

    const fn child(self, kind: Kind) -> Self {
        Self {
            kind,
            stmt: self.stmt,
            direct_stmt: false,
            negated: false,
            wrap: None,
        }
    }

    const fn negated(self, kind: Kind) -> Self {
        Self {
            kind,
            stmt: self.stmt,
            direct_stmt: false,
            negated: true,
            wrap: None,
        }
    }

    const fn wrapping(self, span: Span) -> Self {
        Self {
            wrap: Some(span),
            ..self
        }
    }

    const fn value(self) -> Self {
        self.child(Kind::Value)
    }

    const fn boolean(self) -> Self {
        self.child(Kind::Bool)
    }
}

/// What one walk of a file decided.
pub(super) struct Walked {
    /// The candidates, in the order the walk found them.
    pub(super) found: Vec<Found>,
    /// The skip tallies.
    pub(super) skips: BTreeMap<SkipReason, u32>,
    /// Every decision, in the order the walk took them.
    pub(super) decisions: Vec<Decision>,
    /// Every file this one pastes in.
    pub(super) includes: Vec<Include>,
    /// Every marker the file carries.
    pub(super) annotations: Vec<Claim>,
}

/// A walk whose internal offsets or exact counters contradicted the bounded source established at discovery entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WalkBoundsError;

/// The file being walked.
#[derive(Debug, Clone, Copy)]
pub(super) struct Input<'a> {
    /// The whole text, byte order mark and shebang included.
    pub(super) text: &'a str,
    /// The byte offset the parsed remainder starts at.
    pub(super) base: u32,
    /// The workspace-relative path.
    pub(super) path: &'a str,
    /// The lowercase hex SHA-256 of the bytes.
    pub(super) digest: &'a str,
}

/// One proposed edit.
struct Edit {
    span: Span,
    replacement: Vec<u8>,
    site: Option<Site>,
    /// What a probe of this edit would ask, when one can be stated at all.
    probe: Option<Question>,
}

/// What became of a site.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Candidate(Form),
    Skipped(SkipReason),
}

pub(super) struct Walker<'a> {
    src: &'a str,
    base: u32,
    path: &'a str,
    digest: &'a str,
    selection: &'a Selection<'a>,
    index: LineIndex<'a>,
    found: Vec<Found>,
    skips: BTreeMap<SkipReason, u32>,
    decisions: Vec<Decision>,
    suppressed: Option<SkipReason>,
    frames: Vec<Frame>,
    mod_depth: u32,
    /// What a proof would rest on for each `if` or `while` condition being walked, innermost last.
    gates: Vec<Option<branch::Prepared>>,
    /// The loops being walked, innermost last: the label each carries, and whether its breaks decide its value.
    loops: Vec<(Option<String>, bool)>,
    /// Every `rust-mutants: skip` marker the file carries, in source order.
    markers: Vec<Marker>,
    /// Which markers hid a place a rule targets.
    matched: Vec<bool>,
    /// The marker whose scope the walk is inside, if any.
    annotation: Option<usize>,
    /// The items the walk is inside, outermost first: modules, impls, traits, and the function or constant itself.
    items: Vec<String>,
    includes: Vec<Include>,
    /// Poisoned on the first impossible conversion or counter overflow.
    /// The walk may keep traversing, but [`Self::finish`] then fails closed and releases none of its candidates.
    bounds_failed: Cell<bool>,
}

impl<'a> Walker<'a> {
    pub(super) const fn new(
        input: Input<'a>,
        selection: &'a Selection<'a>,
        index: LineIndex<'a>,
    ) -> Self {
        Self {
            src: input.text,
            base: input.base,
            path: input.path,
            digest: input.digest,
            selection,
            index,
            found: Vec::new(),
            skips: BTreeMap::new(),
            decisions: Vec::new(),
            suppressed: None,
            frames: Vec::new(),
            mod_depth: 0,
            gates: Vec::new(),
            loops: Vec::new(),
            markers: Vec::new(),
            matched: Vec::new(),
            annotation: None,
            items: Vec::new(),
            includes: Vec::new(),
            bounds_failed: Cell::new(false),
        }
    }

    /// The results, unsorted.
    /// Hands the walk the markers it is to honour, before it starts.
    pub(super) fn annotate(&mut self, markers: Vec<Marker>) {
        self.matched = vec![false; markers.len()];
        self.markers = markers;
    }

    pub(super) fn finish(self) -> Result<Walked, WalkBoundsError> {
        if self.bounds_failed.get() {
            return Err(WalkBoundsError);
        }
        let annotations = self
            .markers
            .iter()
            .enumerate()
            .map(|(index, marker)| Claim {
                line: marker.line,
                reason: marker.reason.clone(),
                matched: match self.matched.get(index) {
                    Some(matched) => *matched,
                    None => false,
                },
            })
            .collect();
        Ok(Walked {
            found: self.found,
            skips: self.skips,
            decisions: self.decisions,
            includes: self.includes,
            annotations,
        })
    }

    /// The marker that speaks about a place starting on `line`, if one does.
    fn marker_at(&self, line: u32) -> Option<usize> {
        self.markers.iter().position(|marker| marker.scope == line)
    }

    /// The item the walk is inside, as a reader writes it: `mod::path::Type::method`.
    fn item_path(&self) -> String {
        self.items.join("::")
    }

    /// Walks something under the name it goes by.
    fn within_item(&mut self, name: String, walk: impl FnOnce(&mut Self)) {
        self.items.push(name);
        walk(self);
        if self.items.pop().is_none() {
            self.bounds_failed.set(true);
        }
    }

    /// The line a byte offset sits on.
    fn line_of(&self, offset: u32) -> u32 {
        self.position(offset).line
    }

    fn position(&self, offset: u32) -> super::Position {
        match self.index.position(offset) {
            Ok(position) => position,
            Err(_bounds) => {
                self.bounds_failed.set(true);
                super::Position {
                    line: 1,
                    byte_column: 1,
                    char_column: 1,
                }
            }
        }
    }

    /// The lines a marker may sit above to speak about this construct: where its attributes start, and where the first token after them does.
    fn heading(&self, span: Span, attrs: &[Attribute]) -> [u32; 2] {
        let outer = self.line_of(span.start);
        let Some(last) = attrs.last() else {
            return [outer, outer];
        };
        let after_offset = self.span(last).end;
        let Some(after) = self.usize_offset(after_offset) else {
            return [outer, outer];
        };
        let Some(rest) = self.src.get(after..) else {
            self.bounds_failed.set(true);
            return [outer, outer];
        };
        let Some(skipped) = rest.len().checked_sub(rest.trim_start().len()) else {
            self.bounds_failed.set(true);
            return [outer, outer];
        };
        let Some(skipped) = self.relative_offset(skipped) else {
            return [outer, outer];
        };
        let Some(inner_offset) = after_offset.checked_add(skipped) else {
            self.bounds_failed.set(true);
            return [outer, outer];
        };
        let inner = self.line_of(inner_offset);
        [outer, inner]
    }

    /// Walks a construct under the marker that speaks about it, if one does.
    fn maybe_annotated(&mut self, lines: [u32; 2], walk: impl FnOnce(&mut Self)) {
        let found = lines
            .into_iter()
            .filter_map(|line| self.marker_at(line))
            .find(|index| {
                self.markers
                    .get(*index)
                    .is_some_and(|marker| marker.own_line)
            });
        let Some(index) = found else {
            walk(self);
            return;
        };
        let annotation = self.annotation.replace(index);
        self.with_suppression(SkipReason::Annotated, walk);
        self.annotation = annotation;
    }

    /// Records an `include!`, when its argument names a file this run can name.
    fn record_include(&mut self, mac: &Macro, at_item: bool) {
        if !mac.path.is_ident("include") {
            return;
        }
        let Ok(literal) = syn::parse2::<syn::LitStr>(mac.tokens.clone()) else {
            return;
        };
        if let Some(path) = beside(self.path, &literal.value()) {
            self.includes.push(Include { path, at_item });
        }
    }

    /// Walks a file and reports whether it carries `#![no_std]`.
    pub(super) fn walk_file(&mut self, file: &syn::File) -> bool {
        self.walk_items(&file.items);
        file.attrs.iter().any(|attr| attr.path().is_ident("no_std"))
    }

    /// The absolute span of a node.
    fn span<T: Spanned + ?Sized>(&self, node: &T) -> Span {
        let range = node.span().byte_range();
        let offset = |at: usize| match self.absolute_offset(at) {
            Some(offset) => offset,
            None => 0,
        };
        Span {
            start: offset(range.start),
            end: offset(range.end),
        }
    }

    fn text(&self, span: Span) -> &'a str {
        let (Some(start), Some(end)) = (self.usize_offset(span.start), self.usize_offset(span.end))
        else {
            return "";
        };
        match self.src.get(start..end) {
            Some(text) => text,
            None => {
                self.bounds_failed.set(true);
                ""
            }
        }
    }

    fn usize_offset(&self, value: u32) -> Option<usize> {
        match usize::try_from(value) {
            Ok(value) => Some(value),
            Err(_overflow) => {
                self.bounds_failed.set(true);
                None
            }
        }
    }

    fn relative_offset(&self, value: usize) -> Option<u32> {
        match u32::try_from(value) {
            Ok(value) => Some(value),
            Err(_overflow) => {
                self.bounds_failed.set(true);
                None
            }
        }
    }

    fn absolute_offset(&self, value: usize) -> Option<u32> {
        self.relative_offset(value).and_then(|relative| {
            let absolute = relative.checked_add(self.base);
            if absolute.is_none() {
                self.bounds_failed.set(true);
            }
            absolute
        })
    }

    const fn site_for(ctx: Ctx, own: Span) -> Option<Site> {
        match ctx.kind {
            Kind::Bool => Some(Site {
                form: Form::C,
                span: own,
            }),
            Kind::Value => Some(Site {
                form: Form::E,
                span: own,
            }),
            Kind::Place => None,
        }
    }

    /// Proposes one edit.
    /// Under a suppression it is counted rather than kept; without a site it is an unsupported-site skip.
    fn emit(&mut self, rule_name: &str, edit: Edit) {
        let Some(rule) = self.selection.rule(rule_name) else {
            return;
        };
        if let Some(reason) = self.suppressed {
            self.skip(reason);
            self.decide(edit.span.start, rule_name, Outcome::Skipped(reason));
            return;
        }
        let line = self.line_of(edit.span.start);
        if let Some(index) = self.marker_at(line) {
            let reason = self
                .markers
                .get(index)
                .map_or_else(String::new, |marker| marker.reason.clone());
            if let Some(claimed) = self.matched.get_mut(index) {
                *claimed = true;
            }
            self.declined(
                At::new(edit.span.start, rule_name).noting(&reason),
                SkipReason::Annotated,
            );
            return;
        }
        let Some(site) = edit.site else {
            self.skip(SkipReason::UnsupportedSite);
            self.decide(
                edit.span.start,
                rule_name,
                Outcome::Skipped(SkipReason::UnsupportedSite),
            );
            return;
        };
        let original = self.text(edit.span).as_bytes().to_vec();
        if original == edit.replacement {
            self.declined(
                At::new(edit.span.start, rule_name).noting("identical-replacement"),
                SkipReason::UnsupportedSite,
            );
            return;
        }
        let candidate = Candidate {
            path: self.path.to_owned(),
            rule,
            span: edit.span,
            original,
            replacement: edit.replacement,
            source_digest: self.digest.to_owned(),
        };
        let item = self.item_path();
        let hint = SiteHint {
            form: site.form,
            site: site.span,
            site_text: self.text(site.span).to_owned(),
            super_depth: self.mod_depth,
        };
        let position = self.position(edit.span.start);
        let gate = self.gates.last().and_then(Option::as_ref);
        let branch = gate.and_then(|gate| gate.claim(rule_name, edit.span));
        let comparable = gate.and_then(|gate| gate.comparable(rule_name, edit.span));
        self.found.push(Found {
            candidate,
            position,
            item,
            hint,
            branch,
            comparable,
            probe: edit.probe,
        });
        self.decide(edit.span.start, rule_name, Outcome::Candidate(site.form));
    }

    fn skip(&mut self, reason: SkipReason) {
        if reason == SkipReason::Annotated
            && let Some(index) = self.annotation
            && let Some(claimed) = self.matched.get_mut(index)
        {
            *claimed = true;
        }
        let count = self.skips.entry(reason).or_insert(0);
        match count.checked_add(1) {
            Some(incremented) => *count = incremented,
            None => self.bounds_failed.set(true),
        }
    }

    fn decide(&mut self, offset: u32, rule: &str, outcome: Outcome) {
        self.noted(At::new(offset, rule), outcome);
    }

    /// One decision, with what the walker has to say about it beyond its reason.
    fn noted(&mut self, at: At<'_>, outcome: Outcome) {
        let (form, skip) = match outcome {
            Outcome::Candidate(form) => (Some(form), None),
            Outcome::Skipped(reason) => (None, Some(reason)),
        };
        self.decisions.push(Decision {
            offset: at.offset,
            position: self.position(at.offset),
            rule: at.rule.to_owned(),
            form,
            skip,
            note: at.note.map(ToOwned::to_owned),
        });
    }

    /// One place a rule targeted and passed over, counted and said.
    fn declined(&mut self, at: At<'_>, reason: SkipReason) {
        if self.selection.rule(at.rule).is_none() {
            return;
        }
        self.skip(reason);
        self.noted(at, Outcome::Skipped(reason));
    }

    /// A macro invocation in expression or statement position: the arguments of an assertion, or one skip.
    fn macro_expr(&mut self, mac: &Macro) {
        self.record_include(mac, false);
        if !self.walk_assertion(mac) {
            self.macro_site(mac);
        }
    }

    /// The leading arguments of an assertion macro, walked as the expressions they are.
    /// Answers whether they were.
    fn walk_assertion(&mut self, mac: &Macro) -> bool {
        let Some(arity) = assertion_arity(&mac.path) else {
            return false;
        };
        let split = arguments(&mac.tokens);
        let Some(leading) = split.get(..arity) else {
            return false;
        };
        let parsed: Vec<Expr> = match leading
            .iter()
            .map(|one| syn::parse2::<Expr>(one.clone()))
            .collect::<Result<_, _>>()
        {
            Ok(parsed) => parsed,
            Err(_opaque_macro_tokens) => return false,
        };
        if parsed.len() != arity {
            return false;
        }
        let kind = if assertion_is_condition(&mac.path) {
            Kind::Bool
        } else {
            Kind::Value
        };
        for expr in &parsed {
            self.walk_expr(expr, Ctx::new(kind, None));
        }
        true
    }

    /// An item-position macro invocation, which is where `include!` pastes items.
    fn macro_item(&mut self, mac: &Macro) {
        self.record_include(mac, true);
        self.macro_site(mac);
    }

    /// A macro invocation: one skip, under the outer reason if there is one.
    fn macro_site(&mut self, mac: &Macro) {
        let span = self.span(mac);
        let reason = match self.suppressed {
            Some(reason) => reason,
            None => SkipReason::MacroInvocation,
        };
        self.skip(reason);
        self.decide(
            span.start,
            SkipReason::MacroInvocation.name(),
            Outcome::Skipped(reason),
        );
    }

    fn with_suppression(&mut self, reason: SkipReason, walk: impl FnOnce(&mut Self)) {
        let previous = self.suppressed;
        if previous.is_none() {
            self.suppressed = Some(reason);
        }
        walk(self);
        self.suppressed = previous;
    }

    fn maybe_suppressed(&mut self, attrs: &[Attribute], walk: impl FnOnce(&mut Self)) {
        match suppression_of(attrs) {
            Some(reason) => self.with_suppression(reason, walk),
            None => walk(self),
        }
    }

    fn with_frame(&mut self, frame: Frame, walk: impl FnOnce(&mut Self)) {
        self.frames.push(frame);
        let mut loops = Vec::new();
        std::mem::swap(&mut loops, &mut self.loops);
        walk(self);
        self.loops = loops;
        if self.frames.pop().is_none() {
            self.bounds_failed.set(true);
        }
    }

    fn within_loop(
        &mut self,
        label: Option<&syn::Label>,
        valued: bool,
        walk: impl FnOnce(&mut Self),
    ) {
        self.loops
            .push((label.map(|label| label.name.ident.to_string()), valued));
        walk(self);
        if self.loops.pop().is_none() {
            self.bounds_failed.set(true);
        }
    }

    /// Whether the loop a jump names is one whose breaks decide its value.
    fn breaks_decide_the_value(&self, label: Option<&syn::Lifetime>) -> bool {
        label.map_or_else(
            || self.loops.last().is_none_or(|(_, valued)| *valued),
            |named| {
                let wanted = named.ident.to_string();
                self.loops
                    .iter()
                    .rev()
                    .find(|(label, _)| label.as_deref() == Some(wanted.as_str()))
                    .is_none_or(|(_, valued)| *valued)
            },
        )
    }

    fn walk_items(&mut self, items: &[Item]) {
        for item in items {
            self.walk_item(item);
        }
    }

    fn walk_item(&mut self, item: &Item) {
        let heading = self.heading(self.span(item), item_attrs(item));
        self.maybe_annotated(heading, |walker| {
            walker.maybe_suppressed(item_attrs(item), |walker| walker.walk_item_inner(item));
        });
    }

    fn walk_item_inner(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                self.within_item(name, |walker| walker.walk_fn(&f.sig, &f.block));
            }
            Item::Impl(i) => {
                self.within_item(implemented(i), |walker| {
                    for member in &i.items {
                        walker.walk_impl_item(member);
                    }
                });
            }
            Item::Trait(t) => {
                self.within_item(t.ident.to_string(), |walker| {
                    for member in &t.items {
                        walker.walk_trait_item(member);
                    }
                });
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    let Some(deeper) = self.mod_depth.checked_add(1) else {
                        self.bounds_failed.set(true);
                        return;
                    };
                    self.mod_depth = deeper;
                    self.within_item(m.ident.to_string(), |walker| walker.walk_items(items));
                    match self.mod_depth.checked_sub(1) {
                        Some(shallower) => self.mod_depth = shallower,
                        None => self.bounds_failed.set(true),
                    }
                }
            }
            Item::Const(c) => {
                let name = c.ident.to_string();
                self.within_item(name, |walker| walker.walk_const_expr(&c.expr));
            }
            Item::Static(s) => {
                let name = s.ident.to_string();
                self.within_item(name, |walker| walker.walk_const_expr(&s.expr));
            }
            Item::Enum(e) => {
                for variant in &e.variants {
                    if let Some((_, discriminant)) = &variant.discriminant {
                        self.walk_const_expr(discriminant);
                    }
                }
            }
            Item::Macro(m) if !m.mac.path.is_ident("macro_rules") => self.macro_item(&m.mac),
            _ => {}
        }
    }

    fn walk_impl_item(&mut self, member: &ImplItem) {
        match member {
            ImplItem::Fn(f) => {
                let name = f.sig.ident.to_string();
                self.within_item(name, |walker| {
                    walker.maybe_suppressed(&f.attrs, |walker| {
                        walker.walk_fn(&f.sig, &f.block);
                    });
                });
            }
            ImplItem::Const(c) => {
                let name = c.ident.to_string();
                self.within_item(name, |walker| {
                    walker.maybe_suppressed(&c.attrs, |walker| walker.walk_const_expr(&c.expr));
                });
            }
            ImplItem::Macro(m) => self.macro_site(&m.mac),
            _ => {}
        }
    }

    fn walk_trait_item(&mut self, member: &TraitItem) {
        match member {
            TraitItem::Fn(f) => {
                if let Some(block) = &f.default {
                    let name = f.sig.ident.to_string();
                    self.within_item(name, |walker| {
                        walker.maybe_suppressed(&f.attrs, |walker| {
                            walker.walk_fn(&f.sig, block);
                        });
                    });
                }
            }
            TraitItem::Const(c) => {
                if let Some((_, expr)) = &c.default {
                    let name = c.ident.to_string();
                    self.within_item(name, |walker| {
                        walker.maybe_suppressed(&c.attrs, |walker| walker.walk_const_expr(expr));
                    });
                }
            }
            TraitItem::Macro(m) => self.macro_site(&m.mac),
            _ => {}
        }
    }

    fn walk_fn(&mut self, sig: &Signature, block: &Block) {
        let (generic, defaultable) = parameters(&sig.generics);
        let frame = Frame {
            ret: return_kind_within(&sig.output, &generic, &defaultable),
        };
        if sig.constness.is_some() {
            self.with_suppression(SkipReason::ConstFnBody, |walker| {
                walker.with_frame(frame, |walker| walker.walk_block(block, true));
            });
        } else {
            self.with_frame(frame, |walker| walker.walk_block(block, true));
        }
    }

    fn walk_const_expr(&mut self, expr: &Expr) {
        self.with_suppression(SkipReason::ConstContext, |walker| {
            walker.walk_expr(expr, Ctx::new(Kind::Value, None));
        });
    }

    /// Walks a block; when it is a function body, its tail expression is a return site.
    fn walk_block(&mut self, block: &Block, fn_body: bool) {
        let Some(last) = block.stmts.len().checked_sub(1) else {
            return;
        };
        for (index, stmt) in block.stmts.iter().enumerate() {
            let role = if index != last {
                TailRole::NotLast
            } else if fn_body {
                TailRole::ReturnValue
            } else {
                TailRole::BlockValue
            };
            self.walk_stmt(stmt, role);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt, role: TailRole) {
        match stmt {
            Stmt::Local(local) => {
                let heading = self.heading(self.span(local), &local.attrs);
                self.maybe_annotated(heading, |walker| {
                    walker.maybe_suppressed(&local.attrs, |walker| walker.walk_local(local));
                });
            }
            Stmt::Item(item) => self.walk_item(item),
            Stmt::Expr(expr, Some(semi)) => {
                let stmt_span = Span {
                    start: self.span(expr).start,
                    end: self.span(semi).end,
                };
                let heading = self.heading(stmt_span, expr_attrs(expr));
                self.maybe_annotated(heading, |walker| {
                    walker.maybe_suppressed(expr_attrs(expr), |walker| {
                        walker.statement_candidates(expr, stmt_span);
                        walker.deletable_else(expr, stmt_span);
                        let site = Site {
                            form: Form::S,
                            span: stmt_span,
                        };
                        let mut ctx = Ctx::new(Kind::Value, Some(site));
                        ctx.direct_stmt = true;
                        walker.walk_expr(expr, ctx);
                    });
                });
            }
            Stmt::Expr(expr, None) => {
                let span = self.span(expr);
                let site = Site {
                    form: if role == TailRole::NotLast {
                        Form::S
                    } else {
                        Form::E
                    },
                    span,
                };
                let heading = self.heading(span, expr_attrs(expr));
                self.maybe_annotated(heading, |walker| {
                    walker.maybe_suppressed(expr_attrs(expr), |walker| {
                        if role == TailRole::NotLast {
                            walker.deletable_else(expr, span);
                        }
                        walker.walk_expr(expr, Ctx::new(Kind::Value, Some(site)));
                        if role == TailRole::ReturnValue {
                            walker.return_site(expr);
                        }
                    });
                });
            }
            Stmt::Macro(m) => {
                self.maybe_suppressed(&m.attrs, |walker| walker.macro_expr(&m.mac));
            }
        }
    }

    fn walk_local(&mut self, local: &syn::Local) {
        let Some(init) = &local.init else {
            return;
        };
        let span = self.span(&init.expr);
        let site = Site {
            form: Form::E,
            span,
        };
        self.walk_expr(&init.expr, Ctx::new(Kind::Value, Some(site)));
        if let Some((_, diverge)) = &init.diverge {
            self.walk_expr(diverge, Ctx::new(Kind::Value, None));
        }
    }

    /// The candidates whose edit is the statement itself: deletions and the dropped `?`.
    fn statement_candidates(&mut self, expr: &Expr, stmt_span: Span) {
        let site = Some(Site {
            form: Form::S,
            span: stmt_span,
        });
        match expr {
            Expr::Call(_) | Expr::MethodCall(_) => {
                self.emit(
                    "delete-call-statement",
                    Edit {
                        span: stmt_span,
                        replacement: Vec::new(),
                        site,
                        probe: None,
                    },
                );
            }
            Expr::Try(t) => {
                if matches!(*t.expr, Expr::Call(_) | Expr::MethodCall(_)) {
                    self.emit(
                        "delete-call-statement",
                        Edit {
                            span: stmt_span,
                            replacement: Vec::new(),
                            site,
                            probe: None,
                        },
                    );
                }
                let question = self.span(&t.question_token);
                self.emit(
                    "ignore-question-statement",
                    Edit {
                        span: question,
                        replacement: Vec::new(),
                        site,
                        probe: None,
                    },
                );
            }
            Expr::Assign(_) => self.emit(
                "delete-assignment",
                Edit {
                    span: stmt_span,
                    replacement: Vec::new(),
                    site,
                    probe: None,
                },
            ),
            Expr::Binary(b) if is_compound_assignment(&b.op) => {
                self.emit(
                    "delete-compound-assignment",
                    Edit {
                        span: stmt_span,
                        replacement: Vec::new(),
                        site,
                        probe: None,
                    },
                );
            }
            _ => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr, ctx: Ctx) {
        let attrs = expr_attrs(expr);
        if !attrs.is_empty()
            && !ctx.direct_stmt
            && let Some(reason) = suppression_of(attrs)
        {
            self.with_suppression(reason, |walker| walker.walk_expr_inner(expr, ctx));
            return;
        }
        self.walk_expr_inner(expr, ctx);
    }

    fn walk_expr_inner(&mut self, expr: &Expr, ctx: Ctx) {
        match expr {
            Expr::Binary(b) => self.walk_binary(b, ctx),
            Expr::Unary(u) => self.walk_unary(u, ctx),
            Expr::Lit(lit) => self.walk_literal(lit, ctx),
            Expr::If(i) => self.walk_if(i, ctx),
            Expr::While(w) => self.walk_while(w, ctx),
            Expr::Match(m) => self.walk_match(m, ctx),
            Expr::Return(r) => self.walk_return(r),
            Expr::Try(t) => self.walk_try(t, ctx),
            Expr::Range(r) => self.walk_range(r, ctx),
            Expr::Closure(c) => self.walk_closure(c),
            Expr::Macro(m) => self.macro_expr(&m.mac),
            Expr::Paren(p) => self.walk_expr(&p.expr, ctx.child(ctx.kind)),
            Expr::Group(g) => self.walk_expr(&g.expr, ctx.child(ctx.kind)),
            Expr::Let(l) => self.walk_expr(&l.expr, ctx.value()),
            Expr::Block(b) => self.walk_block(&b.block, false),
            Expr::Unsafe(u) => self.walk_block(&u.block, false),
            Expr::Loop(l) => {
                self.within_loop(l.label.as_ref(), true, |walker| {
                    walker.walk_block(&l.body, false);
                });
            }
            Expr::ForLoop(f) => {
                self.walk_expr(&f.expr, ctx.value());
                self.within_loop(f.label.as_ref(), false, |walker| {
                    walker.walk_block(&f.body, false);
                });
            }
            Expr::Async(a) => {
                let frame = Frame {
                    ret: ReturnKind::Unknown,
                };
                self.with_frame(frame, |walker| walker.walk_block(&a.block, false));
            }
            Expr::TryBlock(t) => {
                let frame = Frame {
                    ret: ReturnKind::Unknown,
                };
                self.with_frame(frame, |walker| walker.walk_block(&t.block, false));
            }
            Expr::Const(c) => {
                self.with_suppression(SkipReason::ConstContext, |walker| {
                    walker.walk_block(&c.block, false);
                });
            }
            Expr::Break(_) | Expr::Continue(_) => self.walk_jump(expr, ctx),
            Expr::Repeat(r) => {
                self.walk_expr(&r.expr, ctx.value());
                self.walk_const_expr(&r.len);
            }
            other => self.walk_compound(other, ctx),
        }
    }

    /// The expressions that only hold other expressions.
    fn walk_compound(&mut self, expr: &Expr, ctx: Ctx) {
        let value = ctx.value();
        match expr {
            Expr::Call(c) => {
                self.walk_expr(&c.func, value);
                for arg in &c.args {
                    self.walk_expr(arg, value);
                }
            }
            Expr::MethodCall(m) => {
                self.walk_method_name(m, ctx);
                let chain = ctx.wrap.unwrap_or_else(|| self.span(m));
                self.walk_expr(&m.receiver, value.wrapping(chain));
                for arg in &m.args {
                    self.walk_expr(arg, value);
                }
            }
            Expr::Index(i) => {
                self.walk_expr(&i.expr, value);
                self.walk_expr(&i.index, value);
            }
            Expr::Field(f) => self.walk_expr(&f.base, value),
            Expr::Reference(r) => self.walk_expr(&r.expr, value),
            Expr::RawAddr(r) => self.walk_expr(&r.expr, value),
            Expr::Cast(c) => self.walk_expr(&c.expr, value),
            Expr::Await(a) => self.walk_expr(&a.base, value),
            Expr::Assign(a) => {
                self.walk_expr(&a.left, ctx.child(Kind::Place));
                self.walk_expr(&a.right, value);
            }
            Expr::Tuple(t) => {
                for elem in &t.elems {
                    self.walk_expr(elem, value);
                }
            }
            Expr::Array(a) => {
                for elem in &a.elems {
                    self.walk_expr(elem, value);
                }
            }
            Expr::Struct(s) => {
                for field in &s.fields {
                    self.walk_expr(&field.expr, value);
                }
                if let Some(rest) = &s.rest {
                    self.walk_expr(rest, value);
                }
            }
            Expr::Break(b) => {
                if let Some(inner) = &b.expr {
                    self.walk_expr(inner, value);
                }
            }
            Expr::Yield(y) => {
                if let Some(inner) = &y.expr {
                    self.walk_expr(inner, value);
                }
            }
            _ => {}
        }
    }

    fn walk_binary(&mut self, b: &syn::ExprBinary, ctx: Ctx) {
        let own = self.span(b);
        if let Some((rule, original, replacement)) = binary_swap(&b.op) {
            let edit = self.span(&b.op);
            let connective_with_let =
                is_connective(&b.op) && (has_let(&b.left) || has_let(&b.right));
            if connective_with_let {
                self.declined(At::new(edit.start, rule), SkipReason::LetCondition);
            } else if self.text(edit) != original {
                self.declined(
                    At::new(edit.start, rule).noting("text-mismatch"),
                    SkipReason::UnsupportedSite,
                );
            } else {
                let site = if is_compound_assignment(&b.op) {
                    if ctx.direct_stmt {
                        ctx.stmt
                    } else {
                        Self::site_for(ctx, own)
                    }
                } else {
                    Self::site_for(ctx, own)
                };
                self.emit(
                    rule,
                    Edit {
                        span: edit,
                        replacement: replacement.as_bytes().to_vec(),
                        site,
                        probe: None,
                    },
                );
            }
        }
        if is_compound_assignment(&b.op) {
            self.walk_expr(&b.left, ctx.child(Kind::Place));
            self.walk_expr(&b.right, ctx.value());
        } else if is_connective(&b.op) {
            self.walk_expr(&b.left, ctx.boolean());
            self.walk_expr(&b.right, ctx.boolean());
        } else {
            self.walk_expr(&b.left, ctx.value());
            self.walk_expr(&b.right, ctx.value());
        }
    }

    /// What a method call offers: the one identifier a swap edits, and the negation of a call that answers a question.
    fn walk_method_name(&mut self, m: &syn::ExprMethodCall, ctx: Ctx) {
        let name = m.method.to_string();
        let own = self.span(m);
        if let Some((rule, replacement)) = method_swap(&name) {
            self.emit(
                rule,
                Edit {
                    span: self.span(&m.method),
                    replacement: replacement.as_bytes().to_vec(),
                    site: Self::site_for(
                        ctx,
                        match ctx.wrap {
                            Some(wrap) => wrap,
                            None => own,
                        },
                    ),
                    probe: None,
                },
            );
        }
        if bool_method(&name) && !ctx.negated {
            let replacement = format!("!({})", self.text(own)).into_bytes();
            self.emit(
                "negate-bool-method",
                Edit {
                    span: own,
                    replacement,
                    site: Self::site_for(ctx, own),
                    probe: None,
                },
            );
        }
    }

    fn walk_unary(&mut self, u: &syn::ExprUnary, ctx: Ctx) {
        let own = self.span(u);
        if let Some(rule) = unary_removal(&u.op) {
            let operand = self.text(self.span(&u.expr)).as_bytes().to_vec();
            self.emit(
                rule,
                Edit {
                    span: own,
                    replacement: operand,
                    site: Self::site_for(ctx, own),
                    probe: None,
                },
            );
            let inner = if is_not(&u.op) {
                ctx.negated(if ctx.kind == Kind::Bool {
                    Kind::Bool
                } else {
                    Kind::Value
                })
            } else {
                ctx.value()
            };
            self.walk_expr(&u.expr, inner);
        } else if matches!(u.op, syn::UnOp::Deref(_)) {
            self.walk_expr(&u.expr, ctx.child(Kind::Place));
        } else {
            self.walk_expr(&u.expr, ctx.value());
        }
    }

    fn walk_literal(&mut self, lit: &syn::ExprLit, ctx: Ctx) {
        let own = self.span(lit);
        let offer = |walker: &mut Self, rule: &str, replacement: String| {
            walker.emit(
                rule,
                Edit {
                    span: own,
                    replacement: replacement.into_bytes(),
                    site: Self::site_for(ctx, own),
                    probe: None,
                },
            );
        };
        match &lit.lit {
            syn::Lit::Bool(b) => {
                let (rule, replacement) = if b.value {
                    ("true-to-false", "false")
                } else {
                    ("false-to-true", "true")
                };
                offer(self, rule, replacement.to_owned());
            }
            syn::Lit::Int(int) => {
                for (rule, delta) in [("int-increment", 1), ("int-decrement", -1)] {
                    if let Some(respelled) = respell_int(int, delta) {
                        offer(self, rule, respelled);
                    }
                }
            }
            syn::Lit::Str(text) if !text.value().is_empty() => {
                offer(self, "string-to-empty", String::from("\"\""));
            }
            _ => {}
        }
    }

    /// The `else` a statement's `if` chain ends with, which a statement can do without.
    fn deletable_else(&mut self, expr: &Expr, stmt_span: Span) {
        let Some((then_branch, otherwise)) = terminal_else(expr) else {
            return;
        };
        let span = Span {
            start: self.span(then_branch).end,
            end: self.span(otherwise).end,
        };
        self.emit(
            "delete-else-branch",
            Edit {
                span,
                replacement: Vec::new(),
                site: Some(Site {
                    form: Form::S,
                    span: stmt_span,
                }),
                probe: None,
            },
        );
    }

    /// A `break` and a `continue` say opposite things about the loop they are in, and either is the other with its label kept.
    fn walk_jump(&mut self, expr: &Expr, ctx: Ctx) {
        let (rule, replacement) = match expr {
            Expr::Break(one) => {
                if let Some(value) = &one.expr {
                    self.declined(
                        At::new(self.span(expr).start, "break-to-continue"),
                        SkipReason::LoopValue,
                    );
                    self.walk_expr(value, ctx.value());
                    return;
                }
                let label = one
                    .label
                    .as_ref()
                    .map_or_else(String::new, |label| format!(" {label}"));
                ("break-to-continue", format!("continue{label}"))
            }
            Expr::Continue(one) => {
                if self.breaks_decide_the_value(one.label.as_ref()) {
                    self.declined(
                        At::new(self.span(expr).start, "continue-to-break"),
                        SkipReason::LoopValue,
                    );
                    return;
                }
                let label = one
                    .label
                    .as_ref()
                    .map_or_else(String::new, |label| format!(" {label}"));
                ("continue-to-break", format!("break{label}"))
            }
            _ => return,
        };
        let own = self.span(expr);
        self.emit(
            rule,
            Edit {
                span: own,
                replacement: replacement.into_bytes(),
                site: Self::site_for(ctx, own),
                probe: None,
            },
        );
    }

    fn negate(&mut self, rule: &str, cond: &Expr) {
        if has_let(cond) {
            return;
        }
        let span = self.span(cond);
        let replacement = format!("!({})", self.text(span)).into_bytes();
        let site = Some(Site {
            form: Form::C,
            span,
        });
        self.emit(
            rule,
            Edit {
                span,
                replacement,
                site,
                probe: None,
            },
        );
    }

    /// Fix a condition at each answer in turn, so a reader is asked about each branch separately.
    fn settle(&mut self, cond: &Expr) {
        if has_let(cond) || is_bool_literal(cond) {
            return;
        }
        let span = self.span(cond);
        let site = Some(Site {
            form: Form::C,
            span,
        });
        for (rule, replacement) in [
            ("condition-to-true", "true"),
            ("condition-to-false", "false"),
        ] {
            self.emit(
                rule,
                Edit {
                    span,
                    replacement: replacement.as_bytes().to_vec(),
                    site,
                    probe: None,
                },
            );
        }
    }

    fn walk_if(&mut self, i: &syn::ExprIf, ctx: Ctx) {
        self.negate("negate-condition", &i.cond);
        self.settle(&i.cond);
        let gate = self.gate(&i.cond, &i.then_branch);
        self.gates.push(gate);
        self.walk_expr(&i.cond, ctx.negated(Kind::Bool));
        self.gates.pop();
        self.walk_block(&i.then_branch, false);
        if let Some((_, else_branch)) = &i.else_branch {
            self.walk_expr(else_branch, ctx.value());
        }
    }

    fn walk_while(&mut self, w: &syn::ExprWhile, ctx: Ctx) {
        self.negate("negate-loop-condition", &w.cond);
        let gate = self.gate(&w.cond, &w.body);
        self.gates.push(gate);
        self.walk_expr(&w.cond, ctx.negated(Kind::Bool));
        self.gates.pop();
        self.within_loop(w.label.as_ref(), false, |walker| {
            walker.walk_block(&w.body, false);
        });
    }

    /// What a proof about this condition would rest on, or nothing when the syntax supports none.
    fn gate(&self, condition: &Expr, body: &Block) -> Option<branch::Prepared> {
        let expr = |e: &Expr| self.span(e);
        let operator = |op: &BinOp| self.span(op);
        branch::prepare(
            branch::Gate {
                condition,
                body: self.span(body),
                statements: body.stmts.len(),
            },
            &branch::Spans {
                expr: &expr,
                operator: &operator,
            },
        )
    }

    fn walk_match(&mut self, m: &syn::ExprMatch, ctx: Ctx) {
        self.walk_expr(&m.expr, ctx.value());
        for (position, arm) in m.arms.iter().enumerate() {
            let deletable = deletable_arm(&m.arms, position);
            let heading = self.heading(self.span(arm), &arm.attrs);
            self.maybe_annotated(heading, |walker| {
                walker.maybe_suppressed(&arm.attrs, |walker| {
                    walker.walk_arm_head(&arm.pat, deletable);
                    walker.walk_pat_guards(&arm.pat, ctx);
                    let span = walker.span(&arm.body);
                    let site = Site {
                        form: Form::E,
                        span,
                    };
                    walker.walk_expr(&arm.body, Ctx::new(Kind::Value, Some(site)));
                });
            });
        }
    }

    /// What an arm's head offers: a guard that can be made false, so the arm is gone, and one that can be made true, so it stops narrowing.
    fn walk_arm_head(&mut self, pat: &Pat, deletable: bool) {
        let Some(guard) = guard_of(pat) else {
            let pattern = self.span(pat);
            if deletable {
                self.emit(
                    "delete-match-arm",
                    Edit {
                        span: Span {
                            start: pattern.end,
                            end: pattern.end,
                        },
                        replacement: format!("{}false", super::ARM_GUARD_OPENING).into_bytes(),
                        site: Some(Site {
                            form: Form::M,
                            span: pattern,
                        }),
                        probe: None,
                    },
                );
            }
            return;
        };
        let span = self.span(guard);
        let site = Some(Site {
            form: Form::C,
            span,
        });
        if deletable {
            self.emit(
                "delete-match-arm",
                Edit {
                    span,
                    replacement: b"false".to_vec(),
                    site,
                    probe: None,
                },
            );
        }
        self.emit(
            "remove-match-guard",
            Edit {
                span,
                replacement: b"true".to_vec(),
                site,
                probe: None,
            },
        );
    }

    fn walk_pat_guards(&mut self, pat: &Pat, ctx: Ctx) {
        match pat {
            Pat::Guard(g) => {
                self.walk_expr(&g.guard, ctx.boolean());
                self.walk_pat_guards(&g.pat, ctx);
            }
            Pat::Paren(p) => self.walk_pat_guards(&p.pat, ctx),
            Pat::Or(o) => {
                for case in &o.cases {
                    self.walk_pat_guards(case, ctx);
                }
            }
            _ => {}
        }
    }

    fn walk_return(&mut self, r: &syn::ExprReturn) {
        let Some(expr) = &r.expr else {
            return;
        };
        self.return_site(expr);
        let span = self.span(expr);
        let site = Site {
            form: Form::E,
            span,
        };
        self.walk_expr(expr, Ctx::new(Kind::Value, Some(site)));
    }

    /// The return-replacement candidates of a returned expression, by what the enclosing function's signature says it returns.
    fn return_site(&mut self, expr: &Expr) {
        let Some(frame) = self.frames.last().copied() else {
            return;
        };
        let span = self.span(expr);
        let offer = ReturnOffer {
            span,
            site: Some(Site {
                form: Form::E,
                span,
            }),
            probeable: crate::probe::is_effect_free(expr),
        };
        match frame.ret {
            ReturnKind::Bool => {
                if !is_true_literal(expr) {
                    self.offer_return(offer, "return-true", "true");
                }
            }
            ReturnKind::Result { ok, err } => {
                self.result_returns(expr, offer, ResultDefaults { ok, err });
            }
            ReturnKind::Option(inner) => self.option_returns(expr, offer, inner),
            ReturnKind::Other => {
                if !is_default_spelling(expr) {
                    self.offer_return(offer, "return-default", "Default::default()");
                }
            }
            ReturnKind::Unstated => self.decline_unstated_returns(span),
            ReturnKind::Unknown | ReturnKind::Unit | ReturnKind::Never => {}
        }
        self.branches_of(expr);
    }

    fn result_returns(&mut self, expr: &Expr, offer: ReturnOffer, defaults: ResultDefaults) {
        if defaults.ok && !is_ok_default(expr) {
            self.offer_return(offer, "return-ok-default", "Ok(Default::default())");
        } else if !defaults.ok {
            self.declined(
                At::new(offer.span.start, "return-ok-default"),
                SkipReason::UnstatedReturnType,
            );
        }
        if defaults.err && !is_err_default(expr) {
            self.offer_return(offer, "return-err-default", "Err(Default::default())");
        } else if !defaults.err {
            self.declined(
                At::new(offer.span.start, "return-err-default"),
                SkipReason::UnstatedReturnType,
            );
        }
    }

    fn option_returns(&mut self, expr: &Expr, offer: ReturnOffer, inner: bool) {
        if !is_default_spelling(expr) {
            self.offer_return(offer, "return-default", "Default::default()");
        }
        if inner && !is_some_default(expr) {
            self.offer_return(offer, "return-some-default", "Some(Default::default())");
        } else if !inner {
            self.declined(
                At::new(offer.span.start, "return-some-default"),
                SkipReason::UnstatedReturnType,
            );
        }
    }

    fn decline_unstated_returns(&mut self, span: Span) {
        for rule in [
            "return-default",
            "return-ok-default",
            "return-some-default",
            "return-err-default",
        ] {
            self.declined(At::new(span.start, rule), SkipReason::UnstatedReturnType);
        }
    }

    fn offer_return(&mut self, offer: ReturnOffer, rule: &str, replacement: &str) {
        self.emit(
            rule,
            Edit {
                span: offer.span,
                replacement: replacement.as_bytes().to_vec(),
                site: offer.site,
                probe: offer
                    .probeable
                    .then(|| Question::of(rule))
                    .and_then(std::convert::identity),
            },
        );
    }

    /// Every branch of a returned `if` or `match` is a place the function returns from too.
    fn branches_of(&mut self, expr: &Expr) {
        match expr {
            Expr::If(one) => {
                if let Some(tail) = block_tail(&one.then_branch) {
                    self.return_site(tail);
                }
                if let Some((_, otherwise)) = &one.else_branch {
                    match otherwise.as_ref() {
                        Expr::Block(block) => {
                            if let Some(tail) = block_tail(&block.block) {
                                self.return_site(tail);
                            }
                        }
                        nested @ Expr::If(_) => self.branches_of(nested),
                        _ => {}
                    }
                }
            }
            Expr::Match(one) => {
                for arm in &one.arms {
                    self.return_site(&arm.body);
                }
            }
            _ => {}
        }
    }

    fn walk_try(&mut self, t: &syn::ExprTry, ctx: Ctx) {
        let own = self.span(t);
        let edit = self.span(&t.question_token);
        self.emit(
            "question-to-unwrap",
            Edit {
                span: edit,
                replacement: b".unwrap()".to_vec(),
                site: Self::site_for(ctx, own),
                probe: None,
            },
        );
        self.walk_expr(&t.expr, ctx.value());
    }

    /// A range swap changes the expression's type, so its site is the statement level the context carries, never the range itself.
    fn walk_range(&mut self, r: &syn::ExprRange, ctx: Ctx) {
        if r.end.is_none() {
            let at = self.span(&r.limits).start;
            for rule in ["range-to-inclusive", "inclusive-to-range"] {
                self.declined(At::new(at, rule), SkipReason::OpenRange);
            }
        }
        if r.end.is_some() {
            let edit = self.span(&r.limits);
            let (rule, replacement) = match r.limits {
                syn::RangeLimits::HalfOpen(_) => ("range-to-inclusive", "..="),
                syn::RangeLimits::Closed(_) => ("inclusive-to-range", ".."),
            };
            self.emit(
                rule,
                Edit {
                    span: edit,
                    replacement: replacement.as_bytes().to_vec(),
                    site: ctx.stmt,
                    probe: None,
                },
            );
        }
        if let Some(start) = &r.start {
            self.walk_expr(start, ctx.value());
        }
        if let Some(end) = &r.end {
            self.walk_expr(end, ctx.value());
        }
    }

    fn walk_closure(&mut self, c: &syn::ExprClosure) {
        let frame = Frame {
            ret: match &c.output {
                ReturnType::Default => ReturnKind::Unknown,
                typed @ ReturnType::Type(..) => return_kind(typed),
            },
        };
        self.with_frame(frame, |walker| match &*c.body {
            Expr::Block(b) => walker.walk_block(&b.block, true),
            body => {
                let span = walker.span(body);
                let site = Site {
                    form: Form::E,
                    span,
                };
                walker.walk_expr(body, Ctx::new(Kind::Value, Some(site)));
                walker.return_site(body);
            }
        });
    }
}
