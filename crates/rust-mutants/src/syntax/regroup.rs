// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether an operator swap is the tree it names: the same operands, grouped as they were, joined by the new operator.

use syn::spanned::Spanned as _;
use syn::visit_mut::VisitMut;
use syn::{BinOp, Expr};

/// How tightly a binary operator binds, loosest first, in the order the Rust reference gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Binding {
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
    /// How tightly `op` binds, or nothing for an operator this release does not know.
    pub(super) const fn of(op: &BinOp) -> Option<Self> {
        Some(match op {
            BinOp::Mul(_) | BinOp::Div(_) | BinOp::Rem(_) => Self::Multiplicative,
            BinOp::Add(_) | BinOp::Sub(_) => Self::Additive,
            BinOp::Shl(_) | BinOp::Shr(_) => Self::Shift,
            BinOp::BitAnd(_) => Self::BitAnd,
            BinOp::BitXor(_) => Self::BitXor,
            BinOp::BitOr(_) => Self::BitOr,
            BinOp::Eq(_)
            | BinOp::Ne(_)
            | BinOp::Lt(_)
            | BinOp::Le(_)
            | BinOp::Gt(_)
            | BinOp::Ge(_) => Self::Compare,
            BinOp::And(_) => Self::And,
            BinOp::Or(_) => Self::Or,
            BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_) => Self::Assign,
            _ => return None,
        })
    }
}

/// Which side of its operator an operand stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    /// Before the operator.
    Left,
    /// After it.
    Right,
}

/// Whether `operand`, written as it is on `side` of an operator binding as `new` does, would be read as a different operand.
/// Only an operator between operands can be regrouped: every other kind of expression binds tighter than any binary operator or had to be parenthesized to stand there at all.
pub(super) fn regroups(operand: &Expr, side: Side, new: Binding) -> bool {
    let Expr::Binary(inner) = operand else {
        return false;
    };
    let Some(inner) = Binding::of(&inner.op) else {
        return true;
    };
    match side {
        Side::Left => inner < new || (inner == new && new == Binding::Compare),
        Side::Right => inner <= new,
    }
}

/// A file with every parenthesis and invisible group taken out, so two files compare equal exactly when they hold the same tree.
pub(super) fn ungrouped(mut file: syn::File) -> syn::File {
    Ungroup.visit_file_mut(&mut file);
    file
}

/// Takes every parenthesis and invisible group out of what it visits.
struct Ungroup;

impl VisitMut for Ungroup {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        loop {
            let inner = match expr {
                Expr::Paren(paren) => (*paren.expr).clone(),
                Expr::Group(group) => (*group.expr).clone(),
                _ => break,
            };
            *expr = inner;
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

/// Replaces the operator of the one binary expression whose operator starts at `at`, and says whether it found one.
struct Swap<'a> {
    at: usize,
    new: &'a BinOp,
    found: bool,
}

impl VisitMut for Swap<'_> {
    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        if binary.op.span().byte_range().start == self.at {
            binary.op = *self.new;
            self.found = true;
        }
        syn::visit_mut::visit_expr_binary_mut(self, binary);
    }
}

/// The tree of one file, ungrouped, which every operator swap in it is held to.
#[derive(Debug)]
pub(super) struct Grouping {
    ungrouped: syn::File,
}

impl Grouping {
    /// The tree `file` holds.
    pub(super) fn of(file: &syn::File) -> Self {
        Self {
            ungrouped: ungrouped(file.clone()),
        }
    }

    /// Whether `written`, the whole file with one edit applied, parses as this file's tree with exactly the operator starting at byte `at` replaced by `new`: the operands it had, grouped as they were.
    pub(super) fn keeps(&self, written: &str, at: usize, new: &BinOp) -> bool {
        let after = match syn::parse_str::<syn::File>(written) {
            Ok(after) => after,
            Err(_does_not_parse) => return false,
        };
        let mut expected = self.ungrouped.clone();
        let mut swap = Swap {
            at,
            new,
            found: false,
        };
        swap.visit_file_mut(&mut expected);
        swap.found && expected == ungrouped(after)
    }
}
