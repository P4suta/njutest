// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a branch proof needs from the syntax alone, before the compiler has vouched for the operands.

use syn::{BinOp, Expr, UnOp};

use crate::span::Span;

/// The rules whose edit can only make a condition less often true.
pub const DECREASING: [&str; 3] = ["le-to-lt", "ge-to-gt", "or-to-and"];

/// Whether `rule` is one of [`DECREASING`].
#[must_use]
pub fn is_decreasing(rule: &str) -> bool {
    DECREASING.contains(&rule)
}

/// The rules whose edit is the negation of what it replaces, so the two can never answer the same.
pub const NEGATING: [&str; 2] = ["eq-to-neq", "neq-to-eq"];

/// Whether `rule` is one of [`NEGATING`].
#[must_use]
pub fn is_negating(rule: &str) -> bool {
    NEGATING.contains(&rule)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proof {
    /// Where the body's opening brace is.
    pub body_start: crate::syntax::Position,
    /// Where its closing brace is.
    pub body_end: crate::syntax::Position,
    /// The marker the instrumenter writes at the body's first statement, when the compiler took one there.
    pub marker: Option<Marker>,
}

/// The call the instrumenter writes at a body's first statement, so entering the body is a thing the guards record.
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
    #[must_use]
    pub fn comparable(&self, rule: &str, edit: Span) -> Option<Comparable> {
        (!is_negating(rule) && self.reachable.contains(&edit)).then(|| Comparable {
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
