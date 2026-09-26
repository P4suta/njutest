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

/// One item as the file reads it where nothing but items encloses it, each kind parsed as its container parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unit {
    /// An item of the file or of an inline module.
    Free(syn::Item),
    /// A member of an `impl` block.
    OfImpl(syn::ImplItem),
    /// A member of a trait.
    OfTrait(syn::TraitItem),
    /// A member of an `extern` block.
    Foreign(syn::ForeignItem),
}

impl Unit {
    /// `text` read as an item of this unit's kind, or nothing where it does not read as one whole.
    fn read_as(&self, text: &str) -> Option<Self> {
        let read = match self {
            Self::Free(_) => syn::parse_str(text).map(Self::Free),
            Self::OfImpl(_) => syn::parse_str(text).map(Self::OfImpl),
            Self::OfTrait(_) => syn::parse_str(text).map(Self::OfTrait),
            Self::Foreign(_) => syn::parse_str(text).map(Self::Foreign),
        };
        match read {
            Ok(unit) => Some(unit),
            Err(_does_not_read) => None,
        }
    }

    /// This unit with every parenthesis and invisible group taken out, so two units compare equal exactly when they hold the same tree.
    fn ungrouped(mut self) -> Self {
        match &mut self {
            Self::Free(item) => Ungroup.visit_item_mut(item),
            Self::OfImpl(item) => Ungroup.visit_impl_item_mut(item),
            Self::OfTrait(item) => Ungroup.visit_trait_item_mut(item),
            Self::Foreign(item) => Ungroup.visit_foreign_item_mut(item),
        }
        self
    }

    /// This unit with the operator starting at byte `at` replaced by `new`, or nothing where no operator starts there.
    fn swapped(&self, at: usize, new: &BinOp) -> Option<Self> {
        let mut swapped = self.clone();
        let mut swap = Swap {
            at,
            new,
            found: false,
        };
        match &mut swapped {
            Self::Free(item) => swap.visit_item_mut(item),
            Self::OfImpl(item) => swap.visit_impl_item_mut(item),
            Self::OfTrait(item) => swap.visit_trait_item_mut(item),
            Self::Foreign(item) => swap.visit_foreign_item_mut(item),
        }
        swap.found.then_some(swapped)
    }

    /// The bytes of the file this unit spans, attributes included.
    fn bytes(&self) -> std::ops::Range<usize> {
        match self {
            Self::Free(item) => item.span().byte_range(),
            Self::OfImpl(item) => item.span().byte_range(),
            Self::OfTrait(item) => item.span().byte_range(),
            Self::Foreign(item) => item.span().byte_range(),
        }
    }
}

/// One unit of the file, where it stands and the tree it holds ungrouped.
#[derive(Debug)]
struct Leaf {
    bytes: std::ops::Range<usize>,
    ungrouped: Unit,
}

impl Leaf {
    /// `unit`, where it stands and ungrouped.
    fn of(unit: Unit) -> Self {
        Self {
            bytes: unit.bytes(),
            ungrouped: unit.ungrouped(),
        }
    }
}

/// Every unit of `items` in file order, descending into what only holds items: inline modules, `impl` blocks, traits and `extern` blocks.
fn leaves(items: &[syn::Item], found: &mut Vec<Leaf>) {
    for item in items {
        match item {
            syn::Item::Mod(module) => match &module.content {
                Some((_, inner)) => leaves(inner, found),
                None => found.push(Leaf::of(Unit::Free(item.clone()))),
            },
            syn::Item::Impl(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::OfImpl(member.clone()))),
            ),
            syn::Item::Trait(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::OfTrait(member.clone()))),
            ),
            syn::Item::ForeignMod(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::Foreign(member.clone()))),
            ),
            other => found.push(Leaf::of(Unit::Free(other.clone()))),
        }
    }
}

/// The units of one file, ungrouped, which every operator swap in it is held to.
///
/// A file is its items read one after another, and each is read by its own tokens up to its own closing brace or semicolon, so an edit inside one unit changes that unit's tree and no other: holding a swap to its unit holds it to the file, at the cost of the unit rather than of the file.
#[derive(Debug)]
pub(super) struct Grouping {
    leaves: Vec<Leaf>,
    read: std::cell::Cell<Option<usize>>,
}

impl Grouping {
    /// The units `file` holds.
    pub(super) fn of(file: &syn::File) -> Self {
        let mut found = Vec::new();
        leaves(&file.items, &mut found);
        Self {
            leaves: found,
            read: std::cell::Cell::new(Some(0)),
        }
    }

    /// How many bytes of source holding every swap has read back, or nothing once that stopped fitting.
    pub(super) const fn read(&self) -> Option<usize> {
        self.read.get()
    }

    /// How many units read alone as the file `text` reads them, and the bytes of every one that does not.
    pub(super) fn read_alone(&self, text: &str) -> (usize, Vec<std::ops::Range<usize>>) {
        let (alike, differing): (Vec<&Leaf>, Vec<&Leaf>) = self.leaves.iter().partition(|leaf| {
            text.get(leaf.bytes.clone())
                .and_then(|unit| leaf.ungrouped.read_as(unit))
                .is_some_and(|read| read.ungrouped() == leaf.ungrouped)
        });
        (
            alike.len(),
            differing
                .into_iter()
                .map(|leaf| leaf.bytes.clone())
                .collect(),
        )
    }

    /// Whether `text` with `edit` rewritten as `written` reads as this file's tree with exactly the operator starting at byte `at` replaced by `new`: the operands it had, grouped as they were.
    /// An edit no single unit holds is one this cannot vouch for, and it says so.
    pub(super) fn keeps(
        &self,
        text: &str,
        (edit, written): (std::ops::Range<usize>, &str),
        (at, new): (usize, &BinOp),
    ) -> bool {
        let after = self
            .leaves
            .partition_point(|leaf| leaf.bytes.end < edit.end);
        let Some(leaf) = self
            .leaves
            .get(after)
            .filter(|leaf| leaf.bytes.start <= edit.start && edit.end <= leaf.bytes.end)
        else {
            return false;
        };
        let (Some(head), Some(tail)) = (
            text.get(leaf.bytes.start..edit.start),
            text.get(edit.end..leaf.bytes.end),
        ) else {
            return false;
        };
        let unit = format!("{head}{written}{tail}");
        self.read.set(
            self.read
                .get()
                .and_then(|read| read.checked_add(unit.len())),
        );
        let (Some(read), Some(expected)) = (
            leaf.ungrouped.read_as(&unit),
            leaf.ungrouped.swapped(at, new),
        ) else {
            return false;
        };
        expected == read.ungrouped()
    }
}
