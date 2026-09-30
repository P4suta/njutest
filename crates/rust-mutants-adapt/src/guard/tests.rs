// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Alternative, Composed, Form, Paths, compose, named};

const PATHS: Paths<'static> = Paths {
    module: "rt",
    depth: 0,
};

const ORIGINAL: &str = "a() || b()";

fn alternative(
    index: u32,
    text: &str,
    comparable: bool,
    probe: Option<&'static str>,
) -> Alternative {
    Alternative {
        index,
        text: text.to_owned(),
        comparable,
        probe,
    }
}

fn plain() -> Alternative {
    alternative(7, "false", false, None)
}

fn compared_and_probed() -> Alternative {
    alternative(8, "b()", true, Some("untrue"))
}

fn composed(form: Form, alternatives: &[Alternative], original: &str) -> Composed {
    let composed = compose(form, &PATHS, alternatives, original);
    for (one, (index, range)) in alternatives.iter().zip(&composed.alternatives) {
        assert_eq!(
            *index, one.index,
            "{form:?}: the spans come in the order given"
        );
        assert_eq!(
            composed.text.get(range.clone()),
            Some(one.text.as_str()),
            "{form:?}: the span of mutant {index} holds its text in {:?}",
            composed.text
        );
    }
    assert_eq!(
        composed.alternatives.len(),
        alternatives.len(),
        "{form:?}: one span per alternative"
    );
    assert_eq!(
        composed
            .text
            .get(composed.original_at..)
            .map(|rest| rest.starts_with(original)),
        Some(true),
        "{form:?}: the original starts where the guard says in {:?}",
        composed.text
    );
    composed
}

#[test]
fn every_form_is_written_as_its_letter() {
    assert_eq!(
        Form::ALL.map(Form::letter),
        ["C", "E", "S", "M"],
        "a form is named by its letter wherever a report names it"
    );
    assert_eq!(
        Form::ALL.map(|form| form.to_string()),
        ["C", "E", "S", "M"],
        "and shown as it"
    );
}

#[test]
fn a_runtime_function_is_named_from_as_deep_as_its_site_stands() {
    assert_eq!(named("rt", 0, "active"), "rt::active");
    assert_eq!(named("rt", 1, "value"), "super::rt::value");
    assert_eq!(named("rt", 2, "body"), "super::super::rt::body");
    assert_eq!(
        Paths {
            module: "__rm",
            depth: 1
        }
        .of("differing"),
        "super::__rm::differing"
    );
}

#[test]
fn a_boolean_selector_holds_every_branch_and_compares_what_may_be_compared() {
    let both = composed(Form::C, &[plain(), compared_and_probed()], ORIGINAL);
    assert_eq!(
        both.text,
        "rt::value!(rt::active(7) && rt::value!(false) || rt::active(8) && rt::value!(b()) || \
         !rt::active(7) && !rt::active(8) && rt::differing(8, rt::value!(a() || b()), || \
         rt::value!(b())))"
    );
    assert_eq!(both.compared, vec![8]);
    let one = composed(Form::C, &[plain()], ORIGINAL);
    assert_eq!(
        one.text,
        "rt::value!(rt::active(7) && rt::value!(false) || !rt::active(7) && rt::value!(a() || \
         b()))"
    );
    assert_eq!(one.compared, Vec::<u32>::new());
    let none = composed(Form::C, &[], ORIGINAL);
    assert_eq!(none.text, "rt::value!(rt::value!(a() || b()))");
    assert_eq!(none.original_at, 22);
    let nested = composed(
        Form::C,
        &[compared_and_probed(), alternative(9, "c", true, None)],
        ORIGINAL,
    );
    assert_eq!(
        nested.text,
        "rt::value!(rt::active(8) && rt::value!(b()) || rt::active(9) && rt::value!(c) || \
         !rt::active(8) && !rt::active(9) && rt::differing(8, rt::value!(rt::differing(9, \
         rt::value!(a() || b()), || rt::value!(c))), || rt::value!(b())))",
        "each comparison holds the next, the first outermost, and each closes with its own \
         alternative"
    );
    assert_eq!(nested.compared, vec![8, 9]);
}

#[test]
fn a_value_chain_asks_every_vouched_probe_around_the_original() {
    let both = composed(Form::E, &[plain(), compared_and_probed()], ORIGINAL);
    assert_eq!(
        both.text,
        "rt::value!(if rt::active(7) { false } else if rt::active(8) { b() } else { rt::untrue(8, \
         a() || b()) })"
    );
    assert_eq!(both.compared, vec![8]);
    let two = composed(
        Form::E,
        &[
            compared_and_probed(),
            alternative(9, "c", false, Some("undefaulted")),
        ],
        ORIGINAL,
    );
    assert_eq!(
        two.text,
        "rt::value!(if rt::active(8) { b() } else if rt::active(9) { c } else { rt::untrue(8, \
         rt::undefaulted(9, a() || b())) })"
    );
    assert_eq!(two.compared, vec![8, 9]);
    let none = composed(Form::E, &[], ORIGINAL);
    assert_eq!(none.text, "rt::value!(a() || b())");
    assert_eq!(none.original_at, 11);
}

#[test]
fn a_value_that_opens_a_block_is_guarded_by_a_chain_holding_every_branch_in_the_macro() {
    for original in [
        "{ a }",
        "match a { _ => b }",
        "if a { b } else { c }",
        "{ a } + b",
        "unsafe { a }",
    ] {
        let opener = alternative(7, "{ a } - b", false, None);
        let composed = composed(Form::E, &[opener], original);
        assert_eq!(
            composed.text,
            format!(
                "if rt::active(7) {{ rt::value!({{ a }} - b) }} else {{ rt::value!({original}) }}"
            ),
            "a guard over a value that opens a block has to end where that did, so it is the \
             chain; and a branch that opens a block starts the block it is written into, so the \
             macro holds it"
        );
        let unguarded = compose(Form::E, &PATHS, &[], original);
        assert_eq!(unguarded.text, original, "nothing to guard is the original");
        assert_eq!(unguarded.original_at, 0);
    }
    let probed = composed(
        Form::E,
        &[alternative(7, "{ a } - b", false, Some("untrue"))],
        "{ a }",
    );
    assert_eq!(
        probed.text, "if rt::active(7) { rt::value!({ a } - b) } else { rt::untrue(7, { a }) }",
        "a probe holds the original as its argument, where no macro is needed"
    );
    let within = composed(
        Form::E,
        &[alternative(7, "{ a } - b", false, None)],
        ORIGINAL,
    );
    assert_eq!(
        within.text,
        "rt::value!(if rt::active(7) { rt::value!({ a } - b) } else { a() || b() })"
    );
}

#[test]
fn a_statement_guard_holds_its_branches_bare_and_compares_nothing() {
    let both = composed(Form::S, &[plain(), compared_and_probed()], "x();");
    assert_eq!(
        both.text,
        "if rt::active(7) { false } else if rt::active(8) { b() } else { x(); }"
    );
    assert_eq!(both.compared, Vec::<u32>::new());
    let block = composed(
        Form::S,
        &[alternative(11, "{ y(); }", false, None)],
        "{ x(); }",
    );
    assert_eq!(
        block.text, "if rt::active(11) { { y(); } } else { { x(); } }",
        "a statement is no value, so the identity macro holds none of it"
    );
    let deleted = composed(Form::S, &[alternative(10, "", false, None)], "x();");
    assert_eq!(deleted.text, "if rt::active(10) { } else { x(); }");
    let none = composed(Form::S, &[], "x();");
    assert_eq!(none.text, "x();");
    assert_eq!(none.original_at, 0);
}

#[test]
fn an_arm_guard_is_written_after_the_pattern_it_keeps() {
    let arm = composed(Form::M, &[plain(), compared_and_probed()], "0");
    assert_eq!(
        arm.text,
        "0 if rt::value!(rt::active(7) && rt::value!(false) || rt::active(8) && rt::value!(b()) || \
         !rt::active(7) && !rt::active(8) && rt::differing(8, rt::value!(true), || \
         rt::value!(b())))"
    );
    assert_eq!(
        arm.original_at, 0,
        "the pattern is the original, and it stands first"
    );
    assert_eq!(arm.compared, vec![8]);
}

#[test]
fn the_original_is_one_operand_whatever_operators_it_holds() {
    let around = |form: Form| match form {
        Form::C => ("rt::value!(", ")"),
        Form::E | Form::S => ("else { ", " }"),
        Form::M => ("", " if "),
    };
    for form in Form::ALL {
        for comparable in [false, true] {
            let composed = compose(
                form,
                &PATHS,
                &[
                    alternative(7, "false", comparable, None),
                    alternative(8, "false", false, None),
                ],
                ORIGINAL,
            );
            let (open, close) = around(form);
            let (before, rest) = composed.text.split_at(composed.original_at);
            assert!(
                before.ends_with(open)
                    && rest
                        .strip_prefix(ORIGINAL)
                        .is_some_and(|after| after.starts_with(close)),
                "form {form:?}, comparable {comparable}: the original must sit alone between \
                 {open:?} and {close:?}, or an operator in it binds to the guard around it and a \
                 live mutant leaves part of the original deciding: {}",
                composed.text
            );
        }
    }
}
