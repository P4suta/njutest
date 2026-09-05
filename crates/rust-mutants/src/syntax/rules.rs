// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each operator token becomes, and which spellings already are the default a return replacement would produce.

use syn::{BinOp, Expr, Lit, UnOp};

/// The rule that swaps `op`, with the token it writes in its place.
pub(super) const fn binary_swap(op: &BinOp) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match op {
        BinOp::Eq(_) => ("eq-to-neq", "==", "!="),
        BinOp::Ne(_) => ("neq-to-eq", "!=", "=="),
        BinOp::Lt(_) => ("lt-to-le", "<", "<="),
        BinOp::Le(_) => ("le-to-lt", "<=", "<"),
        BinOp::Gt(_) => ("gt-to-ge", ">", ">="),
        BinOp::Ge(_) => ("ge-to-gt", ">=", ">"),
        BinOp::And(_) => ("and-to-or", "&&", "||"),
        BinOp::Or(_) => ("or-to-and", "||", "&&"),
        BinOp::Add(_) => ("add-to-sub", "+", "-"),
        BinOp::Sub(_) => ("sub-to-add", "-", "+"),
        BinOp::Mul(_) => ("mul-to-div", "*", "/"),
        BinOp::Div(_) => ("div-to-mul", "/", "*"),
        BinOp::Rem(_) => ("rem-to-mul", "%", "*"),
        BinOp::BitAnd(_) => ("band-to-bor", "&", "|"),
        BinOp::BitOr(_) => ("bor-to-band", "|", "&"),
        BinOp::BitXor(_) => ("xor-to-band", "^", "&"),
        BinOp::Shl(_) => ("shl-to-shr", "<<", ">>"),
        BinOp::Shr(_) => ("shr-to-shl", ">>", "<<"),
        BinOp::AddAssign(_) => ("add-assign-to-sub-assign", "+=", "-="),
        BinOp::SubAssign(_) => ("sub-assign-to-add-assign", "-=", "+="),
        _ => return None,
    })
}

/// Whether `op` is a compound assignment (`+=`, `<<=`, ...), which makes the expression a statement-shaped `()` and never a value to wrap.
pub(super) const fn is_compound_assignment(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

/// Whether `op` is `&&` or `||`.
pub(super) const fn is_connective(op: &BinOp) -> bool {
    matches!(op, BinOp::And(_) | BinOp::Or(_))
}

/// Whether the expression is `!x`.
pub(super) const fn is_not(op: &UnOp) -> bool {
    matches!(op, UnOp::Not(_))
}

/// The last path segment of `expr` when it is a bare path.
fn last_segment(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// A call `name(arg)` with exactly one argument, when `func` ends in `name`.
fn unary_call<'e>(expr: &'e Expr, name: &str) -> Option<&'e Expr> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if call.args.len() != 1 || last_segment(&call.func).as_deref() != Some(name) {
        return None;
    }
    call.args.first()
}

/// Whether `expr` is spelled as the value `Default::default()` would produce, as far as syntax can tell: `0`, `0.0`, `false`, `""`, `()`, `None`, `[]`, `vec![]`, `Default::default()`, `T::default()`, and a zero-argument `T::new()`. A return replacement that would write the same value again is not a mutation, so these produce no candidate. The list is necessarily incomplete; what it misses is an equivalent mutant that survives, never a missed defect.
pub(super) fn is_default_spelling(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(paren) => is_default_spelling(&paren.expr),
        Expr::Group(group) => is_default_spelling(&group.expr),
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int) => int.base10_digits() == "0",
            Lit::Float(float) => float
                .base10_digits()
                .bytes()
                .all(|b| b == b'0' || b == b'.' || b == b'_'),
            Lit::Bool(b) => !b.value,
            Lit::Str(s) => s.value().is_empty(),
            Lit::ByteStr(s) => s.value().is_empty(),
            _ => false,
        },
        Expr::Tuple(tuple) => tuple.elems.is_empty(),
        Expr::Array(array) => array.elems.is_empty(),
        Expr::Path(_) => last_segment(expr).as_deref() == Some("None"),
        Expr::Call(call) => {
            call.args.is_empty()
                && matches!(last_segment(&call.func).as_deref(), Some("default" | "new"))
        }
        Expr::Macro(mac) => mac.mac.path.is_ident("vec") && mac.mac.tokens.is_empty(),
        _ => false,
    }
}

/// Whether `expr` is `Ok(<default>)`.
pub(super) fn is_ok_default(expr: &Expr) -> bool {
    unary_call(expr, "Ok").is_some_and(is_default_spelling)
}

/// Whether `expr` is `Some(<default>)`.
pub(super) fn is_some_default(expr: &Expr) -> bool {
    unary_call(expr, "Some").is_some_and(is_default_spelling)
}

/// Whether `expr` is the literal `true`.
pub(super) fn is_true_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(paren) => is_true_literal(&paren.expr),
        Expr::Lit(lit) => matches!(&lit.lit, Lit::Bool(b) if b.value),
        _ => false,
    }
}

/// Whether a condition holds a `let` anywhere `&&` and parentheses can reach: an `if let`, a `while let`, or a let chain. Such a condition can neither be negated nor have its connective swapped.
pub(super) fn has_let(expr: &Expr) -> bool {
    match expr {
        Expr::Let(_) => true,
        Expr::Paren(paren) => has_let(&paren.expr),
        Expr::Group(group) => has_let(&group.expr),
        Expr::Binary(binary) if is_connective(&binary.op) => {
            has_let(&binary.left) || has_let(&binary.right)
        }
        _ => false,
    }
}
