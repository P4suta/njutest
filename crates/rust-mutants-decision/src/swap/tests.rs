// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec::Vec;

use super::{Binding, Operator, Side, regroups};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Associates {
    Left,
    Right,
    Not,
}

fn in_the_reference(operator: Operator) -> (u8, Associates) {
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

fn read_apart(inner: Operator, side: Side, new: Operator) -> bool {
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

#[test]
fn every_operand_beside_every_operator_is_parenthesized_exactly_where_the_reference_reads_it_apart()
{
    let mut disagreeing = Vec::new();
    for inner in Operator::ALL {
        for side in Side::ALL {
            for new in Operator::ALL {
                let said = regroups(Binding::of(inner), side, Binding::of(new));
                if said != read_apart(inner, side, new) {
                    disagreeing.push((inner, side, new, said));
                }
            }
        }
    }
    assert!(
        disagreeing.is_empty(),
        "{} placements are parenthesized against the reference, the first {:?}",
        disagreeing.len(),
        disagreeing.first()
    );
}

#[test]
fn operators_bind_as_tightly_as_the_reference_ranks_them() {
    for one in Operator::ALL {
        for other in Operator::ALL {
            let ((one_level, _), (other_level, _)) =
                (in_the_reference(one), in_the_reference(other));
            assert_eq!(
                Binding::of(one).cmp(&Binding::of(other)),
                one_level.cmp(&other_level),
                "{one:?} against {other:?}"
            );
        }
    }
}

#[test]
fn a_planted_rule_that_reads_a_comparison_as_associating_is_caught_by_the_reference() {
    let planted = |inner: Binding, side: Side, new: Binding| match side {
        Side::Left => inner < new,
        Side::Right => inner <= new,
    };
    assert!(
        Operator::ALL
            .into_iter()
            .any(|inner| Side::ALL.into_iter().any(|side| {
                Operator::ALL.into_iter().any(|new| {
                    planted(Binding::of(inner), side, Binding::of(new))
                        != read_apart(inner, side, new)
                })
            })),
        "the reference did not catch `a == b == c` written for `(a == b) == c`"
    );
}
