// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The walk over one file's syntax tree.

use std::collections::BTreeMap;

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::{
    Attribute, BinOp, Block, Expr, ImplItem, Item, Macro, Meta, Pat, ReturnType, Signature, Stmt,
    TraitItem, Type, Visibility,
};

use super::branch;
use super::position::LineIndex;
use super::rules::{
    binary_swap, has_let, is_compound_assignment, is_connective, is_default_spelling, is_not,
    is_ok_default, is_some_default, is_true_literal,
};
use super::{Decision, Form, Found, Selection, SiteHint, SkipReason};
use crate::catalog::Candidate;
use crate::probe::form::Question;
use crate::span::Span;

/// What the enclosing function returns, as far as its signature says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnKind {
    /// A closure without a spelled type, an async block: nothing is known.
    Unknown,
    /// `()` or no return type.
    Unit,
    /// `-> !`.
    Never,
    /// `-> bool`.
    Bool,
    /// `-> Result<..>` by its last path segment.
    Result,
    /// `-> Option<..>` by its last path segment.
    Option,
    /// Anything else, where only `Default::default()` can be offered.
    Other,
}

/// Where a statement without a semicolon sits in its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TailRole {
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
    allow_at: Option<u32>,
    ret: ReturnKind,
}

/// A guard site.
#[derive(Debug, Clone, Copy)]
struct Site {
    form: Form,
    span: Span,
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

/// The context an expression is walked in.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    kind: Kind,
    /// The statement-level site, for an edit that changes its expression's type.
    stmt: Option<Site>,
    /// Whether the expression is the whole of an expression statement.
    direct_stmt: bool,
}

impl Ctx {
    const fn new(kind: Kind, stmt: Option<Site>) -> Self {
        Self {
            kind,
            stmt,
            direct_stmt: false,
        }
    }

    const fn child(self, kind: Kind) -> Self {
        Self {
            kind,
            stmt: self.stmt,
            direct_stmt: false,
        }
    }

    const fn value(self) -> Self {
        self.child(Kind::Value)
    }

    const fn boolean(self) -> Self {
        self.child(Kind::Bool)
    }
}

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
    index: &'a LineIndex,
    found: Vec<Found>,
    skips: BTreeMap<SkipReason, u32>,
    decisions: Vec<Decision>,
    suppressed: Option<SkipReason>,
    frames: Vec<Frame>,
    mod_depth: u32,
    /// What a proof would rest on for each `if` or `while` condition being walked, innermost last.
    gates: Vec<Option<branch::Prepared>>,
}

impl<'a> Walker<'a> {
    pub(super) const fn new(
        input: Input<'a>,
        selection: &'a Selection<'a>,
        index: &'a LineIndex,
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
        }
    }

    /// The results, unsorted.
    pub(super) fn finish(self) -> (Vec<Found>, BTreeMap<SkipReason, u32>, Vec<Decision>) {
        (self.found, self.skips, self.decisions)
    }

    /// Walks a file and reports whether it carries `#![no_std]`.
    pub(super) fn walk_file(&mut self, file: &syn::File) -> bool {
        self.walk_items(&file.items);
        file.attrs.iter().any(|attr| attr.path().is_ident("no_std"))
    }

    /// The absolute span of a node.
    fn span<T: Spanned + ?Sized>(&self, node: &T) -> Span {
        let range = node.span().byte_range();
        let offset = |at: usize| {
            u32::try_from(at)
                .unwrap_or(u32::MAX)
                .saturating_add(self.base)
        };
        Span {
            start: offset(range.start),
            end: offset(range.end),
        }
    }

    fn text(&self, span: Span) -> &'a str {
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(usize::MAX);
        self.src.get(start..end).unwrap_or_default()
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

    /// Proposes one edit. Under a suppression it is counted rather than kept; without a site it is an unsupported-site skip.
    fn emit(&mut self, rule_name: &str, edit: Edit) {
        let Some(rule) = self.selection.rule(rule_name) else {
            return;
        };
        if let Some(reason) = self.suppressed {
            self.skip(reason);
            self.decide(edit.span.start, rule_name, Outcome::Skipped(reason));
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
        let hint = SiteHint {
            form: site.form,
            site: site.span,
            site_text: self.text(site.span).to_owned(),
            super_depth: self.mod_depth,
            allow_at: self.frames.last().and_then(|frame| frame.allow_at),
        };
        let position = self.index.position(self.src, edit.span.start);
        let branch = self
            .gates
            .last()
            .and_then(Option::as_ref)
            .and_then(|gate| gate.claim(rule_name, edit.span));
        self.found.push(Found {
            candidate,
            position,
            hint,
            branch,
            probe: edit.probe,
        });
        self.decide(edit.span.start, rule_name, Outcome::Candidate(site.form));
    }

    fn skip(&mut self, reason: SkipReason) {
        let count = self.skips.entry(reason).or_insert(0);
        *count = count.saturating_add(1);
    }

    fn decide(&mut self, offset: u32, rule: &str, outcome: Outcome) {
        let (form, skip) = match outcome {
            Outcome::Candidate(form) => (Some(form), None),
            Outcome::Skipped(reason) => (None, Some(reason)),
        };
        self.decisions.push(Decision {
            offset,
            position: self.index.position(self.src, offset),
            rule: rule.to_owned(),
            form,
            skip,
        });
    }

    /// A macro invocation: one skip, under the outer reason if there is one.
    fn macro_site(&mut self, mac: &Macro) {
        let span = self.span(mac);
        let reason = self.suppressed.unwrap_or(SkipReason::MacroInvocation);
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
        walk(self);
        self.frames.pop();
    }

    fn inherited_allow(&self) -> Option<u32> {
        self.frames.last().and_then(|frame| frame.allow_at)
    }

    fn walk_items(&mut self, items: &[Item]) {
        for item in items {
            self.walk_item(item);
        }
    }

    fn walk_item(&mut self, item: &Item) {
        self.maybe_suppressed(item_attrs(item), |walker| walker.walk_item_inner(item));
    }

    fn walk_item_inner(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => {
                let start = self.allow_offset(Some(&f.vis), &f.sig);
                self.walk_fn(&f.sig, &f.block, start);
            }
            Item::Impl(i) => {
                for member in &i.items {
                    self.walk_impl_item(member);
                }
            }
            Item::Trait(t) => {
                for member in &t.items {
                    self.walk_trait_item(member);
                }
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    self.mod_depth = self.mod_depth.saturating_add(1);
                    self.walk_items(items);
                    self.mod_depth = self.mod_depth.saturating_sub(1);
                }
            }
            Item::Const(c) => self.walk_const_expr(&c.expr),
            Item::Static(s) => self.walk_const_expr(&s.expr),
            Item::Enum(e) => {
                for variant in &e.variants {
                    if let Some((_, discriminant)) = &variant.discriminant {
                        self.walk_const_expr(discriminant);
                    }
                }
            }
            Item::Macro(m) if !m.mac.path.is_ident("macro_rules") => self.macro_site(&m.mac),
            _ => {}
        }
    }

    fn walk_impl_item(&mut self, member: &ImplItem) {
        match member {
            ImplItem::Fn(f) => {
                let start = self.allow_offset(Some(&f.vis), &f.sig);
                self.maybe_suppressed(&f.attrs, |walker| walker.walk_fn(&f.sig, &f.block, start));
            }
            ImplItem::Const(c) => {
                self.maybe_suppressed(&c.attrs, |walker| walker.walk_const_expr(&c.expr));
            }
            ImplItem::Macro(m) => self.macro_site(&m.mac),
            _ => {}
        }
    }

    fn walk_trait_item(&mut self, member: &TraitItem) {
        match member {
            TraitItem::Fn(f) => {
                if let Some(block) = &f.default {
                    let start = self.allow_offset(None, &f.sig);
                    self.maybe_suppressed(&f.attrs, |walker| walker.walk_fn(&f.sig, block, start));
                }
            }
            TraitItem::Const(c) => {
                if let Some((_, expr)) = &c.default {
                    self.maybe_suppressed(&c.attrs, |walker| walker.walk_const_expr(expr));
                }
            }
            TraitItem::Macro(m) => self.macro_site(&m.mac),
            _ => {}
        }
    }

    /// Where the allow attribute goes for a function: before its visibility if it has one, and otherwise before its signature. Never before the item's attributes, so a doc comment keeps its own line and the attribute lands on the line the reader expects it on.
    fn allow_offset(&self, vis: Option<&Visibility>, sig: &Signature) -> u32 {
        match vis {
            Some(Visibility::Public(token)) => self.span(token).start,
            Some(Visibility::Restricted(restricted)) => self.span(restricted).start,
            Some(Visibility::Inherited) | None => self.span(sig).start,
        }
    }

    fn walk_fn(&mut self, sig: &Signature, block: &Block, item_start: u32) {
        let frame = Frame {
            allow_at: Some(item_start),
            ret: return_kind(&sig.output),
        };
        if sig.constness.is_some() {
            self.with_suppression(SkipReason::ConstContext, |walker| {
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
        let last = block.stmts.len().saturating_sub(1);
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
                self.maybe_suppressed(&local.attrs, |walker| walker.walk_local(local));
            }
            Stmt::Item(item) => self.walk_item(item),
            Stmt::Expr(expr, Some(semi)) => {
                let stmt_span = Span {
                    start: self.span(expr).start,
                    end: self.span(semi).end,
                };
                self.maybe_suppressed(expr_attrs(expr), |walker| {
                    walker.statement_candidates(expr, stmt_span);
                    let site = Site {
                        form: Form::S,
                        span: stmt_span,
                    };
                    let mut ctx = Ctx::new(Kind::Value, Some(site));
                    ctx.direct_stmt = true;
                    walker.walk_expr(expr, ctx);
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
                self.maybe_suppressed(expr_attrs(expr), |walker| {
                    walker.walk_expr(expr, Ctx::new(Kind::Value, Some(site)));
                    if role == TailRole::ReturnValue {
                        walker.return_site(expr);
                    }
                });
            }
            Stmt::Macro(m) => self.maybe_suppressed(&m.attrs, |walker| walker.macro_site(&m.mac)),
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
            Expr::Macro(m) => self.macro_site(&m.mac),
            Expr::Paren(p) => self.walk_expr(&p.expr, ctx.child(ctx.kind)),
            Expr::Group(g) => self.walk_expr(&g.expr, ctx.child(ctx.kind)),
            Expr::Let(l) => self.walk_expr(&l.expr, ctx.value()),
            Expr::Block(b) => self.walk_block(&b.block, false),
            Expr::Unsafe(u) => self.walk_block(&u.block, false),
            Expr::Loop(l) => self.walk_block(&l.body, false),
            Expr::ForLoop(f) => {
                self.walk_expr(&f.expr, ctx.value());
                self.walk_block(&f.body, false);
            }
            Expr::Async(a) => {
                let frame = Frame {
                    allow_at: self.inherited_allow(),
                    ret: ReturnKind::Unknown,
                };
                self.with_frame(frame, |walker| walker.walk_block(&a.block, false));
            }
            Expr::TryBlock(t) => {
                let frame = Frame {
                    allow_at: self.inherited_allow(),
                    ret: ReturnKind::Unknown,
                };
                self.with_frame(frame, |walker| walker.walk_block(&t.block, false));
            }
            Expr::Const(c) => {
                self.with_suppression(SkipReason::ConstContext, |walker| {
                    walker.walk_block(&c.block, false);
                });
            }
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
                self.walk_expr(&m.receiver, value);
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
            if !connective_with_let && self.text(edit) == original {
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

    fn walk_unary(&mut self, u: &syn::ExprUnary, ctx: Ctx) {
        let own = self.span(u);
        if is_not(&u.op) {
            let operand = self.text(self.span(&u.expr)).as_bytes().to_vec();
            self.emit(
                "remove-not",
                Edit {
                    span: own,
                    replacement: operand,
                    site: Self::site_for(ctx, own),
                    probe: None,
                },
            );
            let inner = if ctx.kind == Kind::Bool {
                ctx.boolean()
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
        if let syn::Lit::Bool(b) = &lit.lit {
            let own = self.span(lit);
            let (rule, replacement) = if b.value {
                ("true-to-false", "false")
            } else {
                ("false-to-true", "true")
            };
            self.emit(
                rule,
                Edit {
                    span: own,
                    replacement: replacement.as_bytes().to_vec(),
                    site: Self::site_for(ctx, own),
                    probe: None,
                },
            );
        }
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

    fn walk_if(&mut self, i: &syn::ExprIf, ctx: Ctx) {
        self.negate("negate-condition", &i.cond);
        let gate = self.gate(&i.cond, &i.then_branch);
        self.gates.push(gate);
        self.walk_expr(&i.cond, ctx.boolean());
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
        self.walk_expr(&w.cond, ctx.boolean());
        self.gates.pop();
        self.walk_block(&w.body, false);
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
        for arm in &m.arms {
            self.maybe_suppressed(&arm.attrs, |walker| {
                walker.walk_pat_guards(&arm.pat, ctx);
                let span = walker.span(&arm.body);
                let site = Site {
                    form: Form::E,
                    span,
                };
                walker.walk_expr(&arm.body, Ctx::new(Kind::Value, Some(site)));
            });
        }
    }

    /// The guard of an arm is a boolean position; patterns hold no other runtime expression.
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
        let site = Some(Site {
            form: Form::E,
            span,
        });
        let probeable = crate::probe::form::is_effect_free(expr);
        let offer = |walker: &mut Self, rule: &str, replacement: &str| {
            walker.emit(
                rule,
                Edit {
                    span,
                    replacement: replacement.as_bytes().to_vec(),
                    site,
                    probe: probeable.then(|| Question::of(rule)).flatten(),
                },
            );
        };
        match frame.ret {
            ReturnKind::Bool => {
                if !is_true_literal(expr) {
                    offer(self, "return-true", "true");
                }
            }
            ReturnKind::Result => {
                if !is_ok_default(expr) {
                    offer(self, "return-ok-default", "Ok(Default::default())");
                }
            }
            ReturnKind::Option => {
                if !is_default_spelling(expr) {
                    offer(self, "return-default", "Default::default()");
                }
                if !is_some_default(expr) {
                    offer(self, "return-some-default", "Some(Default::default())");
                }
            }
            ReturnKind::Other => {
                if !is_default_spelling(expr) {
                    offer(self, "return-default", "Default::default()");
                }
            }
            ReturnKind::Unknown | ReturnKind::Unit | ReturnKind::Never => {}
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
            allow_at: self.inherited_allow(),
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

/// What a signature says the function returns.
fn return_kind(output: &ReturnType) -> ReturnKind {
    match output {
        ReturnType::Default => ReturnKind::Unit,
        ReturnType::Type(_, ty) => return_kind_of(ty),
    }
}

fn return_kind_of(ty: &Type) -> ReturnKind {
    match ty {
        Type::Tuple(t) if t.elems.is_empty() => ReturnKind::Unit,
        Type::Never(_) => ReturnKind::Never,
        Type::Paren(p) => return_kind_of(&p.elem),
        Type::Group(g) => return_kind_of(&g.elem),
        Type::Path(p) => match p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .as_deref()
        {
            Some("bool") => ReturnKind::Bool,
            Some("Result") => ReturnKind::Result,
            Some("Option") => ReturnKind::Option,
            _ => ReturnKind::Other,
        },
        _ => ReturnKind::Other,
    }
}

/// The reason attributes suppress what they decorate: `#[test]` and `#[bench]` are test code, a `cfg` mentioning `test` is test code, and any other `cfg` is a configuration the walker does not evaluate.
fn suppression_of(attrs: &[Attribute]) -> Option<SkipReason> {
    let mut cfg = None;
    for attr in attrs {
        let path = attr.path();
        if path.is_ident("test") || path.is_ident("bench") {
            return Some(SkipReason::TestCode);
        }
        if path.is_ident("cfg") {
            if let Meta::List(list) = &attr.meta
                && mentions_test(&list.tokens)
            {
                return Some(SkipReason::TestCode);
            }
            cfg = Some(SkipReason::CfgAttribute);
        }
    }
    cfg
}

fn mentions_test(tokens: &TokenStream) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        TokenTree::Ident(ident) => ident == "test",
        TokenTree::Group(group) => mentions_test(&group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn expr_attrs(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Array(e) => &e.attrs,
        Expr::Assign(e) => &e.attrs,
        Expr::Async(e) => &e.attrs,
        Expr::Await(e) => &e.attrs,
        Expr::Binary(e) => &e.attrs,
        Expr::Block(e) => &e.attrs,
        Expr::Break(e) => &e.attrs,
        Expr::Call(e) => &e.attrs,
        Expr::Cast(e) => &e.attrs,
        Expr::Closure(e) => &e.attrs,
        Expr::Const(e) => &e.attrs,
        Expr::Continue(e) => &e.attrs,
        Expr::Field(e) => &e.attrs,
        Expr::ForLoop(e) => &e.attrs,
        Expr::Group(e) => &e.attrs,
        Expr::If(e) => &e.attrs,
        Expr::Index(e) => &e.attrs,
        Expr::Infer(e) => &e.attrs,
        Expr::Let(e) => &e.attrs,
        Expr::Lit(e) => &e.attrs,
        Expr::Loop(e) => &e.attrs,
        Expr::Macro(e) => &e.attrs,
        Expr::Match(e) => &e.attrs,
        Expr::MethodCall(e) => &e.attrs,
        Expr::Paren(e) => &e.attrs,
        Expr::Path(e) => &e.attrs,
        Expr::Range(e) => &e.attrs,
        Expr::RawAddr(e) => &e.attrs,
        Expr::Reference(e) => &e.attrs,
        Expr::Repeat(e) => &e.attrs,
        Expr::Return(e) => &e.attrs,
        Expr::Struct(e) => &e.attrs,
        Expr::Try(e) => &e.attrs,
        Expr::TryBlock(e) => &e.attrs,
        Expr::Tuple(e) => &e.attrs,
        Expr::Unary(e) => &e.attrs,
        Expr::Unsafe(e) => &e.attrs,
        Expr::While(e) => &e.attrs,
        Expr::Yield(e) => &e.attrs,
        _ => &[],
    }
}
