// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The second opinion about a waived arm, which has to be able to disagree with the person who waived it.

use xtask::lints::declared_enums;
use xtask::shapes::{Shape, shapes};

fn read(source: &str) -> Vec<Shape> {
    let ours = declared_enums(source);
    shapes(source, &ours)
        .into_values()
        .map(|waived| waived.shape)
        .collect()
}

#[test]
fn a_body_that_is_the_answer_for_the_rest_selects() {
    let source = r"
        enum Decision { Tests, Block }
        fn narrow(decision: Decision) -> bool {
            match decision {
                Decision::Tests => true,
                _ => false,
            }
        }
    ";
    assert_eq!(
        read(source),
        vec![Shape::Selects],
        "a literal standing for everything left is the reading somebody meant, and a hint \
         that calls it a defect is one people stop reading"
    );
}

#[test]
fn a_body_that_hands_out_one_of_our_own_verdicts_decides() {
    let source = r"
        enum Outcome { Killed, Survived, Errored }
        fn said(outcome: Outcome) -> Outcome {
            match outcome {
                Outcome::Killed => Outcome::Killed,
                _ => Outcome::Errored,
            }
        }
    ";
    assert_eq!(
        read(source),
        vec![Shape::Decides],
        "`_ => None` and `_ => Outcome::Errored` are the same shape to anything that only \
         asks whether the body looks simple. Telling them apart is the whole of what this \
         is worth: one answers for the rest, the other gives the rest somebody else's verdict"
    );
}

#[test]
fn a_path_that_is_not_one_of_ours_still_selects() {
    let source = r"
        enum Decision { Tests, Block }
        fn found(decision: Decision) -> Option<u8> {
            match decision {
                Decision::Tests => Some(1),
                _ => None,
            }
        }
    ";
    assert_eq!(
        read(source),
        vec![Shape::Selects],
        "`None` is the rest said once, not a verdict, and the difference is whether the \
         head of the path names a set this repository closes"
    );
}

#[test]
fn a_body_that_stops_is_a_conversion_nobody_did_rather_than_a_third_category() {
    let source = r#"
        enum Decision { Tests, Block }
        fn only(decision: Decision) -> u8 {
            match decision {
                Decision::Tests => 1,
                _ => panic!("not this one"),
            }
        }
    "#;
    assert_eq!(read(source), vec![Shape::Refuses]);
    assert!(
        Shape::Refuses.hint().contains("let-else"),
        "an arm that stops is safe and is also a match the compiler could have checked. \
         Said as a third legitimate category it sits there; said as a conversion somebody \
         has not done, it gets done: {}",
        Shape::Refuses.hint()
    );
}

#[test]
fn every_shape_says_the_reading_it_came_from() {
    for shape in [Shape::Selects, Shape::Refuses, Shape::Decides] {
        assert!(
            shape.hint().starts_with("by body shape"),
            "this is a second reading of code somebody else has already read, and one that \
         does not say what it read is one a person can mistake for a verdict: {}",
            shape.hint()
        );
    }
}

#[test]
fn an_arm_over_a_set_nobody_here_closes_is_not_read_at_all() {
    let source = r"
        fn kind(expression: &syn::Expr) -> u8 {
            match expression {
                syn::Expr::Lit(_) => 1,
                _ => 0,
            }
        }
    ";
    assert!(
        read(source).is_empty(),
        "the values of a foreign enum are not ours to list, so an arm standing for the rest \
         of them is the handling rather than a default, and a hint with an opinion about it \
         is a hint with a false positive rate"
    );
}
