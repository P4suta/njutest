// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a branch proof needs from the syntax alone, before the compiler has vouched for the operands.
//!
//! rust-mutants states a proof for an edit that can only make the condition of
//! an `if` or a `while` less often true. Write C for the original condition and
//! C′ for the mutated one: C′ implies C, and the whole condition is inert — no
//! effects, no possible panic, guaranteed to terminate. A test during which no
//! statement of the gated body ran evaluated C to false every time, evaluated
//! C′ to false there too, and ran identically on the two programs. It cannot
//! have observed the mutation.
//!
//! Inertness is decided here as far as syntax can decide it: identifiers,
//! literals, `!`, `&&`, `||`, parentheses, comparisons, and casts. What syntax
//! cannot decide is whether a comparison's operands are primitives — `a < b`
//! on a user type is a call, which may do anything — so each comparison and
//! each cast leaves a [`Witness`] for the compiler to accept or refuse
//! (ADR 0008).
//!
//! The same inertness answers a second question. A guard holds both readings
//! of its site, and where the whole condition is inert an edit on one of the
//! operator tokens the connectives reach leaves it inert — the same operands
//! under another operator of the same class — so a run may evaluate both and
//! record whether they ever parted. That is the infection question asked
//! without a second tree and without a second run:
//! [ADR 0015](../../../../docs/adr/0015-the-guard-is-the-infection-probe.md).

use syn::{BinOp, Expr, UnOp};

use crate::span::Span;

/// The rules whose edit can only make a condition less often true.
pub const DECREASING: [&str; 3] = ["le-to-lt", "ge-to-gt", "or-to-and"];

/// Whether `rule` is one of [`DECREASING`].
#[must_use]
pub fn is_decreasing(rule: &str) -> bool {
    DECREASING.contains(&rule)
}

/// What the compiler must vouch for before a claim becomes a proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum WitnessKind {
    /// Both operands of a comparison are one primitive type that compares without running any of the program's code.
    Ordered,
    /// The operand of a cast is a primitive, so the cast runs none of the program's code.
    Primitive,
}

impl WitnessKind {
    /// The name of the runtime function that states this witness.
    #[must_use]
    pub const fn function(self) -> &'static str {
        match self {
            Self::Ordered => "w_ord",
            Self::Primitive => "w_prim",
        }
    }
}

/// One thing the compiler must vouch for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witness {
    /// What must hold.
    pub kind: WitnessKind,
    /// The bytes of each operand, in source order.
    pub operands: Vec<Span>,
}

/// A branch proof the compiler has vouched for: the span of the body the narrowed condition gates.
///
/// A target during which no statement of this body ran cannot have observed
/// the mutation, and may be discharged from its reaching set without being
/// executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proof {
    /// Where the body's opening brace is.
    pub body_start: crate::syntax::Position,
    /// Where its closing brace is.
    pub body_end: crate::syntax::Position,
    /// The marker the instrumenter writes at the body's first statement, when the compiler took one there.
    ///
    /// The premise this proof needs is that the body did not run, and the two
    /// ways of establishing it are a coverage region beginning inside the body
    /// and a marker the body's own first statement calls. The marker is exact
    /// where a region is inferred, and it needs no coverage build; a body no
    /// marker could go into — a `const` block, a body the compiler refused the
    /// call in — keeps the region as its only premise.
    pub marker: Option<Marker>,
}

/// The call the instrumenter writes at a body's first statement, so entering the body is a thing the guards record.
///
/// It is written at the byte just past the opening brace, on that line, and
/// names the lowest of the mutants whose claim this body carries — one index
/// out of the catalog's own numbering, so the log that carries it needs no
/// second one to bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Marker {
    /// The byte offset the call is written at, which is just past the body's opening brace.
    pub at: u32,
    /// The index the call names.
    pub index: u32,
    /// How many `super::` segments separate the body's inline module from the file root, where the runtime lives.
    pub super_depth: u32,
}

/// What a guard needs before it may evaluate both of its branches, pending the compiler's word on the witnesses.
///
/// It carries the whole condition rather than the edit's own site because the
/// witnesses are written in front of the condition, which is the one place
/// every operand of them is in scope; a claim about the same condition is
/// written in front of the same bytes, so the two questions are one rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparable {
    /// The whole condition, which is what makes evaluating either branch run none of the program's code.
    pub condition: Span,
    /// What the compiler must vouch for.
    pub witnesses: Vec<Witness>,
}

/// A branch proof the syntax supports, pending the compiler's word on its witnesses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The whole condition the edit sits in.
    pub condition: Span,
    /// The body the condition gates, from its opening brace to its closing brace.
    pub body: Span,
    /// What the compiler must vouch for.
    pub witnesses: Vec<Witness>,
}

/// How the caller measures what it parsed. The walker already knows; this module only asks.
#[derive(Clone, Copy)]
pub struct Spans<'a> {
    /// The bytes one expression covers.
    pub expr: &'a dyn Fn(&Expr) -> Span,
    /// The bytes one operator token covers, which is narrower than the gap between its operands.
    pub operator: &'a dyn Fn(&BinOp) -> Span,
}

impl std::fmt::Debug for Spans<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Spans")
    }
}

/// The condition of an `if` or a `while`, and the body it gates.
#[derive(Debug, Clone, Copy)]
pub struct Gate<'a> {
    /// The condition.
    pub condition: &'a Expr,
    /// The body's brace-to-brace span.
    pub body: Span,
    /// How many statements the body holds. A body that runs nothing says nothing by not running.
    pub statements: usize,
}

/// What one gate offers a decreasing edit inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The whole condition.
    pub condition: Span,
    /// The body the condition gates.
    pub body: Span,
    /// What the compiler must vouch for.
    pub witnesses: Vec<Witness>,
    /// The operator tokens an edit may sit on: those reached from the condition through nothing but `&&`, `||`, `!`, and parentheses. An edit anywhere else is not one this proof is about.
    pub reachable: Vec<Span>,
    /// How many statements the body holds, which is what makes a target's silence about it mean something.
    pub statements: usize,
}

impl Prepared {
    /// What the compiler must vouch for before the two branches of the guard at `edit` may be compared, or nothing where the syntax does not allow comparing them.
    ///
    /// A guard has both branches in it: the mutation and what it replaces. If
    /// the whole condition is inert and the edit is one of the operator tokens
    /// the connectives reach, then the mutated condition is inert too — the
    /// same operands, another operator of the same class — and evaluating it
    /// beside the original runs none of the program's code. A run can then
    /// record whether the two ever differed, which is the infection question
    /// asked without a second tree and without a second run of anything.
    ///
    /// The witnesses are the condition's own: what makes it inert is what
    /// makes evaluating it twice inert.
    #[must_use]
    pub fn comparable(&self, edit: Span) -> Option<Comparable> {
        self.reachable.contains(&edit).then(|| Comparable {
            condition: self.condition,
            witnesses: self.witnesses.clone(),
        })
    }

    /// The claim an edit at `edit` supports, or nothing when the edit is not one this gate proves anything about.
    #[must_use]
    pub fn claim(&self, rule: &str, edit: Span) -> Option<Claim> {
        (self.statements > 0 && is_decreasing(rule) && self.reachable.contains(&edit)).then(|| {
            Claim {
                condition: self.condition,
                body: self.body,
                witnesses: self.witnesses.clone(),
            }
        })
    }
}

/// What `gate` offers, or nothing when the syntax supports nothing there at all.
///
/// Nothing is offered unless the whole condition is inert as far as syntax can
/// say and an edit can sit on one of the operator tokens its connectives
/// reach. Whether the body it gates holds a statement is a question only
/// [`Prepared::claim`] asks: a body that runs nothing says nothing by not
/// running, while [`Prepared::comparable`] is about the condition alone and
/// does not care what the body is.
#[must_use]
pub fn prepare(gate: Gate<'_>, spans: &Spans<'_>) -> Option<Prepared> {
    let mut witnesses = Vec::new();
    if !inert(gate.condition, spans, &mut witnesses) {
        return None;
    }
    let mut reachable = Vec::new();
    reach(gate.condition, spans, &mut reachable);
    (!reachable.is_empty()).then(|| Prepared {
        condition: (spans.expr)(gate.condition),
        body: gate.body,
        statements: gate.statements,
        witnesses,
        reachable,
    })
}

/// Every operator token reached from the condition through nothing but the connectives, `!`, and parentheses.
fn reach(expr: &Expr, spans: &Spans<'_>, found: &mut Vec<Span>) {
    match expr {
        Expr::Paren(inner) => reach(&inner.expr, spans, found),
        Expr::Group(inner) => reach(&inner.expr, spans, found),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => {
            reach(&unary.expr, spans, found);
        }
        Expr::Binary(binary) if is_connective(&binary.op) => {
            found.push((spans.operator)(&binary.op));
            reach(&binary.left, spans, found);
            reach(&binary.right, spans, found);
        }
        Expr::Binary(binary) if is_comparison(&binary.op) => {
            found.push((spans.operator)(&binary.op));
        }
        _ => {}
    }
}

/// Whether the whole condition runs none of the program's code, collecting what the compiler must vouch for along the way.
fn inert(expr: &Expr, spans: &Spans<'_>, witnesses: &mut Vec<Witness>) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Paren(inner) => inert(&inner.expr, spans, witnesses),
        Expr::Group(inner) => inert(&inner.expr, spans, witnesses),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => {
            inert(&unary.expr, spans, witnesses)
        }
        Expr::Cast(cast) => {
            witnesses.push(Witness {
                kind: WitnessKind::Primitive,
                operands: vec![(spans.expr)(&cast.expr)],
            });
            inert(&cast.expr, spans, witnesses)
        }
        Expr::Binary(binary) if is_connective(&binary.op) => {
            inert(&binary.left, spans, witnesses) && inert(&binary.right, spans, witnesses)
        }
        Expr::Binary(binary) if is_comparison(&binary.op) => {
            witnesses.push(Witness {
                kind: WitnessKind::Ordered,
                operands: vec![(spans.expr)(&binary.left), (spans.expr)(&binary.right)],
            });
            inert(&binary.left, spans, witnesses) && inert(&binary.right, spans, witnesses)
        }
        _ => false,
    }
}

const fn is_connective(op: &BinOp) -> bool {
    matches!(op, BinOp::And(_) | BinOp::Or(_))
}

const fn is_comparison(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq(_) | BinOp::Ne(_) | BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_)
    )
}
