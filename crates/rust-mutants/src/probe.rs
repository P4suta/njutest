// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which return replacements a guard may ask about, and what it asks.
//!
//! A return replacement writes a constant — the default, `true`,
//! `Ok(default)`, `Some(default)` — so whether a test could have seen it is
//! answered by comparing the value the program already computed against that
//! constant. A value that already equals it changed nothing the test could
//! see, and the test cannot have killed the mutant however far it ran
//! afterwards.
//!
//! Two things have to hold, and this module decides the first. Evaluating the
//! value must not itself be an event — no effects, no possible panic,
//! guaranteed to terminate — which is an allowlist over the syntax,
//! deliberately narrow. The second is the type, which only the compiler can
//! answer: [`crate::instrument::witness`] puts the question, and the sealed
//! trait the witness tree and the runtime are both rendered from says what an
//! answer of yes means.
//!
//! A question this release cannot state is a mutant executed the ordinary way,
//! which costs time and never correctness.

use syn::{BinOp, Expr, Lit, UnOp};

/// The rules a probe can be stated for, and what the probe asks about each.
pub const PROBED: [&str; 4] = [
    "return-default",
    "return-ok-default",
    "return-some-default",
    "return-true",
];

/// Whether `rule` is one a probe can be stated for.
#[must_use]
pub fn is_probed(rule: &str) -> bool {
    PROBED.contains(&rule)
}

/// Whether evaluating `expr` a second time is not itself an event.
///
/// The allowlist is paths, literals, field chains, references, casts, `!`,
/// comparisons, the connectives, tuple and struct literals, the unit
/// constructors, and `Default::default()`. Everything else is refused,
/// including every call and every arithmetic operator: in a debug build `a + b`
/// can panic, and a panic during a probe is an event the unprobed program does
/// not have.
#[must_use]
pub fn is_effect_free(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Paren(inner) => is_effect_free(&inner.expr),
        Expr::Group(inner) => is_effect_free(&inner.expr),
        Expr::Reference(inner) => is_effect_free(&inner.expr),
        Expr::Field(field) => is_effect_free(&field.base),
        Expr::Cast(cast) => is_effect_free(&cast.expr),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => is_effect_free(&unary.expr),
        Expr::Binary(binary) if is_safe_operator(&binary.op) => {
            is_effect_free(&binary.left) && is_effect_free(&binary.right)
        }
        Expr::Tuple(tuple) => tuple.elems.iter().all(is_effect_free),
        Expr::Array(array) => array.elems.iter().all(is_effect_free),
        Expr::Struct(structure) => structure
            .fields
            .iter()
            .all(|field| is_effect_free(&field.expr)),
        Expr::Call(call) => is_constructor(call),
        _ => false,
    }
}

/// Whether the call is one of the unit constructors or `Default::default()`, which run none of the program's own code beyond what their arguments do.
fn is_constructor(call: &syn::ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    let named = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<String>>()
        .join("::");
    let is_wrapper = matches!(named.as_str(), "Some" | "Ok" | "Err");
    let is_default = named.ends_with("Default::default") || named == "default";
    (is_wrapper || is_default) && call.args.iter().all(is_effect_free)
}

/// Whether comparing or combining with this operator runs none of the program's own code beyond what its operands do. Arithmetic is refused: in a debug build it can panic.
const fn is_safe_operator(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq(_)
            | BinOp::Ne(_)
            | BinOp::Lt(_)
            | BinOp::Le(_)
            | BinOp::Gt(_)
            | BinOp::Ge(_)
            | BinOp::And(_)
            | BinOp::Or(_)
    )
}

/// What a probe asks about one return replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Question {
    /// Whether the value already equals what `Default::default()` would produce.
    Default,
    /// Whether the value is `Err`, or an `Ok` already holding the default.
    OkDefault,
    /// Whether the value is `None`, or a `Some` already holding the default.
    SomeDefault,
    /// Whether the value is already `true`.
    True,
}

impl Question {
    /// The question a probe of `rule` asks, when the rule is one a probe can be stated for.
    #[must_use]
    pub fn of(rule: &str) -> Option<Self> {
        match rule {
            "return-default" => Some(Self::Default),
            "return-ok-default" => Some(Self::OkDefault),
            "return-some-default" => Some(Self::SomeDefault),
            "return-true" => Some(Self::True),
            _ => None,
        }
    }

    /// The wire name a trace and a report use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "is-default",
            Self::OkDefault => "is-ok-default",
            Self::SomeDefault => "is-some-default",
            Self::True => "is-true",
        }
    }

    /// The runtime function a guard wraps its original branch in to answer this.
    #[must_use]
    pub const fn runtime(self) -> &'static str {
        match self {
            Self::Default => "undefaulted",
            Self::OkDefault => "unokdefault",
            Self::SomeDefault => "unsomedefault",
            Self::True => "untrue",
        }
    }

    /// The witness function that puts this question's type to the compiler.
    #[must_use]
    pub const fn witness(self) -> &'static str {
        match self {
            Self::Default => "w_default",
            Self::OkDefault => "w_ok_default",
            Self::SomeDefault => "w_some_default",
            Self::True => "w_true",
        }
    }
}

/// Whether the literal is one a probe can be stated about at all. A float is refused outright: `-0.0 == 0.0` holds and `-0.0` is not what `Default::default()` writes, so a probe would say the mutation changed nothing when it changed the sign of a zero.
#[must_use]
pub const fn is_probeable_literal(lit: &Lit) -> bool {
    !matches!(lit, Lit::Float(_))
}
