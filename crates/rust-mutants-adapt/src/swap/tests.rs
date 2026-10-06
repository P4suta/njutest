// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use rust_mutants_decision::swap::{Binding, Operator, Side};
use syn::{BinOp, Expr};

use super::{binding, operator, regroups};

fn written(operator: Operator) -> BinOp {
    match operator {
        Operator::Mul => BinOp::Mul(syn::token::Star::default()),
        Operator::Div => BinOp::Div(syn::token::Slash::default()),
        Operator::Rem => BinOp::Rem(syn::token::Percent::default()),
        Operator::Add => BinOp::Add(syn::token::Plus::default()),
        Operator::Sub => BinOp::Sub(syn::token::Minus::default()),
        Operator::Shl => BinOp::Shl(syn::token::Shl::default()),
        Operator::Shr => BinOp::Shr(syn::token::Shr::default()),
        Operator::BitAnd => BinOp::BitAnd(syn::token::And::default()),
        Operator::BitXor => BinOp::BitXor(syn::token::Caret::default()),
        Operator::BitOr => BinOp::BitOr(syn::token::Or::default()),
        Operator::Eq => BinOp::Eq(syn::token::EqEq::default()),
        Operator::Ne => BinOp::Ne(syn::token::Ne::default()),
        Operator::Lt => BinOp::Lt(syn::token::Lt::default()),
        Operator::Le => BinOp::Le(syn::token::Le::default()),
        Operator::Gt => BinOp::Gt(syn::token::Gt::default()),
        Operator::Ge => BinOp::Ge(syn::token::Ge::default()),
        Operator::And => BinOp::And(syn::token::AndAnd::default()),
        Operator::Or => BinOp::Or(syn::token::OrOr::default()),
        Operator::AddAssign => BinOp::AddAssign(syn::token::PlusEq::default()),
        Operator::SubAssign => BinOp::SubAssign(syn::token::MinusEq::default()),
        Operator::MulAssign => BinOp::MulAssign(syn::token::StarEq::default()),
        Operator::DivAssign => BinOp::DivAssign(syn::token::SlashEq::default()),
        Operator::RemAssign => BinOp::RemAssign(syn::token::PercentEq::default()),
        Operator::BitXorAssign => BinOp::BitXorAssign(syn::token::CaretEq::default()),
        Operator::BitAndAssign => BinOp::BitAndAssign(syn::token::AndEq::default()),
        Operator::BitOrAssign => BinOp::BitOrAssign(syn::token::OrEq::default()),
        Operator::ShlAssign => BinOp::ShlAssign(syn::token::ShlEq::default()),
        Operator::ShrAssign => BinOp::ShrAssign(syn::token::ShrEq::default()),
    }
}

fn unit() -> Expr {
    Expr::Tuple(syn::ExprTuple {
        attrs: Vec::new(),
        paren_token: syn::token::Paren::default(),
        elems: syn::punctuated::Punctuated::new(),
    })
}

fn joined(op: BinOp) -> Expr {
    Expr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(unit()),
        op,
        right: Box::new(unit()),
    })
}

#[test]
fn every_operator_syn_writes_is_the_one_it_names() {
    for one in Operator::ALL {
        assert_eq!(operator(written(one)), Some(one), "{one:?}");
        assert_eq!(
            binding(written(one)),
            Some(Binding::of(one)),
            "{one:?} binds as the decision ranks it"
        );
    }
}

#[test]
fn an_operand_is_regrouped_only_where_it_is_an_operator_the_new_one_reads_apart() {
    for new in Binding::ALL {
        for side in Side::ALL {
            assert!(
                !regroups(&unit(), side, new),
                "an operand that is no operator cannot be regrouped: {side:?} of {new:?}"
            );
            for inner in Operator::ALL {
                assert_eq!(
                    regroups(&joined(written(inner)), side, new),
                    rust_mutants_decision::swap::regroups(Binding::of(inner), side, new),
                    "{inner:?} on the {side:?} of {new:?}"
                );
            }
        }
    }
}

#[test]
fn a_planted_mapping_that_confuses_two_operators_is_caught() {
    let planted = |op: BinOp| match op {
        BinOp::Shl(_) => Some(Operator::Add),
        other => operator(other),
    };
    assert!(
        Operator::ALL
            .into_iter()
            .any(|one| planted(written(one)) != Some(one)),
        "the table did not catch `<<` read as `+`"
    );
}
