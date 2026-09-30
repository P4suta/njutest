// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Binding, Operator, Side, regroups};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Associates {
    Left,
    Right,
    Not,
}

fn symbolic_operator() -> Operator {
    let index = kani::any::<u8>();
    kani::assume(index < 28);
    match index {
        0 => Operator::Mul,
        1 => Operator::Div,
        2 => Operator::Rem,
        3 => Operator::Add,
        4 => Operator::Sub,
        5 => Operator::Shl,
        6 => Operator::Shr,
        7 => Operator::BitAnd,
        8 => Operator::BitXor,
        9 => Operator::BitOr,
        10 => Operator::Eq,
        11 => Operator::Ne,
        12 => Operator::Lt,
        13 => Operator::Le,
        14 => Operator::Gt,
        15 => Operator::Ge,
        16 => Operator::And,
        17 => Operator::Or,
        18 => Operator::AddAssign,
        19 => Operator::SubAssign,
        20 => Operator::MulAssign,
        21 => Operator::DivAssign,
        22 => Operator::RemAssign,
        23 => Operator::BitXorAssign,
        24 => Operator::BitAndAssign,
        25 => Operator::BitOrAssign,
        26 => Operator::ShlAssign,
        _ => Operator::ShrAssign,
    }
}

fn symbolic_side() -> Side {
    if kani::any() { Side::Left } else { Side::Right }
}

const fn in_the_reference(operator: Operator) -> (u8, Associates) {
    match operator {
        Operator::Mul | Operator::Div | Operator::Rem => (10, Associates::Left),
        Operator::Add | Operator::Sub => (9, Associates::Left),
        Operator::Shl | Operator::Shr => (8, Associates::Left),
        Operator::BitAnd => (7, Associates::Left),
        Operator::BitXor => (6, Associates::Left),
        Operator::BitOr => (5, Associates::Left),
        Operator::Eq | Operator::Ne | Operator::Lt | Operator::Le | Operator::Gt | Operator::Ge => {
            (4, Associates::Not)
        }
        Operator::And => (3, Associates::Left),
        Operator::Or => (2, Associates::Left),
        Operator::AddAssign
        | Operator::SubAssign
        | Operator::MulAssign
        | Operator::DivAssign
        | Operator::RemAssign
        | Operator::BitXorAssign
        | Operator::BitAndAssign
        | Operator::BitOrAssign
        | Operator::ShlAssign
        | Operator::ShrAssign => (1, Associates::Right),
    }
}

const fn read_apart(inner: Operator, side: Side, new: Operator) -> bool {
    let ((inner, _), (new, associates)) = (in_the_reference(inner), in_the_reference(new));
    if inner != new {
        return inner < new;
    }
    match (associates, side) {
        (Associates::Not, Side::Left | Side::Right)
        | (Associates::Left, Side::Right)
        | (Associates::Right, Side::Left) => true,
        (Associates::Left, Side::Left) | (Associates::Right, Side::Right) => false,
    }
}

#[kani::proof]
fn a_swap_regroups_as_the_reference_reads() {
    let (inner, side, new) = (symbolic_operator(), symbolic_side(), symbolic_operator());
    let regrouped = regroups(Binding::of(inner), side, Binding::of(new));
    kani::assert(
        regrouped == read_apart(inner, side, new),
        "njutest-law-assertion:regroups-as-the-reference-reads",
    );
    kani::cover!(regrouped, "njutest-law-branch:regrouped");
    kani::cover!(!regrouped, "njutest-law-branch:kept");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn operators_bind_as_the_reference_ranks() {
    let (one, other) = (symbolic_operator(), symbolic_operator());
    let ((one_level, _), (other_level, _)) = (in_the_reference(one), in_the_reference(other));
    kani::assert(
        Binding::of(one).cmp(&Binding::of(other)) == one_level.cmp(&other_level),
        "njutest-law-assertion:binds-as-the-reference-ranks",
    );
    kani::cover!(true, "njutest-law-reached");
}
