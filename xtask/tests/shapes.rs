// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The second opinion about a waived arm, which has to be able to disagree with the person who waived it.

use njutest_devkit::result::{ResultState, result_state};
use xtask::lints::declared_enums;
use xtask::shapes::{Shape, shapes};

fn read(source: &str) -> Vec<Shape> {
    let ours = enums(source);
    let parsed = shapes(source, &ours);
    assert_eq!(
        result_state(&parsed),
        ResultState::Returned,
        "the literal source did not parse: {parsed:?}"
    );
    match parsed {
        Ok(waivers) => waivers,
        Err(_already_reported) => return Vec::new(),
    }
    .into_values()
    .map(|waived| waived.shape)
    .collect()
}

fn enums(source: &str) -> Vec<String> {
    let parsed = declared_enums(source);
    assert_eq!(
        result_state(&parsed),
        ResultState::Returned,
        "the literal source did not parse: {parsed:?}"
    );
    match parsed {
        Ok(enums) => enums,
        Err(_already_reported) => Vec::new(),
    }
}

fn wildcard_lines(source: &str) -> Vec<usize> {
    let parsed = xtask::lints::wildcards(source, &enums(source));
    assert_eq!(
        result_state(&parsed),
        ResultState::Returned,
        "the literal source did not parse: {parsed:?}"
    );
    match parsed {
        Ok(lines) => lines,
        Err(_already_reported) => Vec::new(),
    }
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
    for shape in Shape::ALL {
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

#[test]
fn a_guard_on_the_only_arm_naming_our_set_does_not_hide_the_match() {
    let source = r"
        enum Message { Compiler { error: bool }, Finished { ok: bool } }
        fn first(message: &Message, other: syn::Expr) -> Option<u8> {
            match message {
                Message::Compiler { error } if *error => Some(1),
                syn::Expr::Lit(_) => None,
                _ => None,
            }
        }
    ";
    assert_eq!(
        wildcard_lines(source),
        vec![7],
        "syn 3 keeps a guard inside the pattern rather than beside it, so an arm written          `Message::Compiler {{ .. }} if error` is a Pat::Guard and a walk reading only the          outer shape learns nothing from it. Every guarded match in the workspace was          invisible to this gate, which is a gate that was off and said it was on"
    );
    let untyped = r"
        enum Message { Compiler { error: bool }, Finished { ok: bool } }
        fn first(next: impl Fn() -> Message, other: syn::Expr) -> Option<u8> {
            match next() {
                Message::Compiler { error } if error => Some(1),
                syn::Expr::Lit(_) => None,
                _ => None,
            }
        }
    ";
    assert_eq!(
        wildcard_lines(untyped),
        vec![7],
        "with no typed binding to fall back on, the guarded arm is the only thing naming the \
         set, so this is the match a reader that skips a guard never sees"
    );
}

#[test]
fn an_arm_the_compiler_demands_is_not_a_waiver_anybody_could_have_refused() {
    let every_arm_guarded = r"
        enum Message { Compiler { error: bool }, Finished { ok: bool } }
        fn first(message: &Message) -> Option<u8> {
            match message {
                Message::Compiler { error } if *error => Some(1),
                _ => None,
            }
        }
    ";
    assert!(
        wildcard_lines(every_arm_guarded).is_empty(),
        "no variant is covered unconditionally, so the compiler asks for this arm. A gate \
         demanding a reviewed waiver for it asks somebody to have decided what they could \
         not decide, and a ledger of those is one nobody reads"
    );

    let one_arm_bare = r"
        enum Message { Compiler { error: bool }, Finished { ok: bool } }
        fn first(message: &Message) -> Option<u8> {
            match message {
                Message::Compiler { error } if *error => Some(1),
                Message::Finished { ok } => Some(u8::from(*ok)),
                _ => None,
            }
        }
    ";
    assert_eq!(
        wildcard_lines(one_arm_bare),
        vec![7],
        "one unguarded naming arm and the exemption stops: this errs toward a ledger line \
         rather than toward a blind spot, because the cost of the first is a line somebody \
         reads and the cost of the second is a gate that is off and says it is on"
    );
}

#[test]
fn malformed_source_is_never_an_empty_inventory() {
    let malformed = "fn (";
    let enums = declared_enums(malformed);
    assert_eq!(
        result_state(&enums),
        ResultState::Refused,
        "malformed source declared enums: {enums:?}"
    );
    let wildcards = xtask::lints::wildcards(malformed, &[]);
    assert_eq!(
        result_state(&wildcards),
        ResultState::Refused,
        "malformed source declared wildcard lines: {wildcards:?}"
    );
    let shapes = shapes(malformed, &[]);
    assert_eq!(
        result_state(&shapes),
        ResultState::Refused,
        "malformed source declared shapes: {shapes:?}"
    );
}
