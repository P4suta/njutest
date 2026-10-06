// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether an operand of a swapped operator has to be parenthesized to stay the operand it was, by how tightly Rust binds each binary operator.

/// A binary operator, as Rust writes it between its two operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Operator {
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Rem,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `<<`.
    Shl,
    /// `>>`.
    Shr,
    /// `&`.
    BitAnd,
    /// `^`.
    BitXor,
    /// `|`.
    BitOr,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `+=`.
    AddAssign,
    /// `-=`.
    SubAssign,
    /// `*=`.
    MulAssign,
    /// `/=`.
    DivAssign,
    /// `%=`.
    RemAssign,
    /// `^=`.
    BitXorAssign,
    /// `&=`.
    BitAndAssign,
    /// `|=`.
    BitOrAssign,
    /// `<<=`.
    ShlAssign,
    /// `>>=`.
    ShrAssign,
}

/// How tightly a binary operator binds, loosest first, in the order the Rust reference gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Binding {
    /// `=` and every compound assignment, which associate to the right.
    Assign,
    /// `||`.
    Or,
    /// `&&`.
    And,
    /// `==`, `!=`, `<`, `<=`, `>`, `>=`, which do not associate at all.
    Compare,
    /// `|`.
    BitOr,
    /// `^`.
    BitXor,
    /// `&`.
    BitAnd,
    /// `<<` and `>>`.
    Shift,
    /// `+` and `-`.
    Additive,
    /// `*`, `/` and `%`.
    Multiplicative,
}

impl Binding {
    /// How tightly `operator` binds.
    #[must_use]
    pub const fn of(operator: Operator) -> Self {
        match operator {
            Operator::Mul | Operator::Div | Operator::Rem => Self::Multiplicative,
            Operator::Add | Operator::Sub => Self::Additive,
            Operator::Shl | Operator::Shr => Self::Shift,
            Operator::BitAnd => Self::BitAnd,
            Operator::BitXor => Self::BitXor,
            Operator::BitOr => Self::BitOr,
            Operator::Eq
            | Operator::Ne
            | Operator::Lt
            | Operator::Le
            | Operator::Gt
            | Operator::Ge => Self::Compare,
            Operator::And => Self::And,
            Operator::Or => Self::Or,
            Operator::AddAssign
            | Operator::SubAssign
            | Operator::MulAssign
            | Operator::DivAssign
            | Operator::RemAssign
            | Operator::BitXorAssign
            | Operator::BitAndAssign
            | Operator::BitOrAssign
            | Operator::ShlAssign
            | Operator::ShrAssign => Self::Assign,
        }
    }
}

/// Which side of its operator an operand stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Side {
    /// Before the operator.
    Left,
    /// After it.
    Right,
}

/// Whether an operand whose own operator binds as `inner` does, written unparenthesized on `side` of an operator binding as `new` does, would be read as a different operand.
///
/// It is where it binds more loosely than `new`, and where it binds as tightly, on the side `new`'s level does not group toward: comparisons group toward neither, assignments toward the right, and every other level toward the left.
#[must_use]
pub fn regroups(inner: Binding, side: Side, new: Binding) -> bool {
    match inner.cmp(&new) {
        core::cmp::Ordering::Less => true,
        core::cmp::Ordering::Greater => false,
        core::cmp::Ordering::Equal => match new {
            Binding::Compare => true,
            Binding::Assign => matches!(side, Side::Left),
            Binding::Or
            | Binding::And
            | Binding::BitOr
            | Binding::BitXor
            | Binding::BitAnd
            | Binding::Shift
            | Binding::Additive
            | Binding::Multiplicative => matches!(side, Side::Right),
        },
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
