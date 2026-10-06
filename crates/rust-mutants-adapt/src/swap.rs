// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which operator a `syn` tree writes, and whether an operand in it can be regrouped at all, for the rule `rust_mutants_decision::swap` decides by.

use rust_mutants_decision::swap::{Binding, Operator, Side};
use syn::{BinOp, Expr};

/// The operator `op` writes, or nothing for an operator this release does not know.
#[must_use]
pub const fn operator(op: BinOp) -> Option<Operator> {
    Some(match op {
        BinOp::Mul(_) => Operator::Mul,
        BinOp::Div(_) => Operator::Div,
        BinOp::Rem(_) => Operator::Rem,
        BinOp::Add(_) => Operator::Add,
        BinOp::Sub(_) => Operator::Sub,
        BinOp::Shl(_) => Operator::Shl,
        BinOp::Shr(_) => Operator::Shr,
        BinOp::BitAnd(_) => Operator::BitAnd,
        BinOp::BitXor(_) => Operator::BitXor,
        BinOp::BitOr(_) => Operator::BitOr,
        BinOp::Eq(_) => Operator::Eq,
        BinOp::Ne(_) => Operator::Ne,
        BinOp::Lt(_) => Operator::Lt,
        BinOp::Le(_) => Operator::Le,
        BinOp::Gt(_) => Operator::Gt,
        BinOp::Ge(_) => Operator::Ge,
        BinOp::And(_) => Operator::And,
        BinOp::Or(_) => Operator::Or,
        BinOp::AddAssign(_) => Operator::AddAssign,
        BinOp::SubAssign(_) => Operator::SubAssign,
        BinOp::MulAssign(_) => Operator::MulAssign,
        BinOp::DivAssign(_) => Operator::DivAssign,
        BinOp::RemAssign(_) => Operator::RemAssign,
        BinOp::BitXorAssign(_) => Operator::BitXorAssign,
        BinOp::BitAndAssign(_) => Operator::BitAndAssign,
        BinOp::BitOrAssign(_) => Operator::BitOrAssign,
        BinOp::ShlAssign(_) => Operator::ShlAssign,
        BinOp::ShrAssign(_) => Operator::ShrAssign,
        _ => return None,
    })
}

/// How tightly `op` binds, or nothing for an operator this release does not know.
#[must_use]
pub const fn binding(op: BinOp) -> Option<Binding> {
    match operator(op) {
        Some(operator) => Some(Binding::of(operator)),
        None => None,
    }
}

/// Whether `operand`, written as it is on `side` of an operator binding as `new` does, would be read as a different operand.
///
/// Only an operator between operands can be regrouped: every other kind of expression binds tighter than any binary operator or had to be parenthesized to stand there at all, and an operator this release does not know is held to be one that can.
#[must_use]
pub fn regroups(operand: &Expr, side: Side, new: Binding) -> bool {
    let Expr::Binary(inner) = operand else {
        return false;
    };
    binding(inner.op).is_none_or(|inner| rust_mutants_decision::swap::regroups(inner, side, new))
}

#[cfg(test)]
mod tests;
