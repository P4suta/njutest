// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run establishes about one wire fault, from what the tests did with it in place.

use njutest_cli::report::SeamDecision;
use njutest_cli::wire::derive::Fault;
use njutest_cli::wire::rule::Rule;
use njutest_cli::wire::settle::{Answered, settle};

fn fault() -> Fault {
    Fault {
        id: "f".repeat(64),
        capability: "api".to_owned(),
        seq: 0,
        during: Some("pkg/test/it".to_owned()),
        rule: Rule::StatusServerError,
    }
}

/// What the suite said, as the thing `settle` is given.
fn put(answers: &[Answered]) -> njutest_cli::wire::settle::Asked {
    njutest_cli::wire::settle::Asked::Answered(answers.to_vec())
}

fn answered(target: &str, passed: bool) -> Answered {
    Answered {
        target: target.to_owned(),
        passed,
    }
}

#[test]
fn a_fault_nothing_ran_is_one_the_run_established_nothing_about() {
    let settled = settle(&fault(), &put(&[]));
    assert_eq!(
        settled.decision,
        SeamDecision::Unreached,
        "a fault the run never put to anything is a hole rather than a survivor: \
         calling it survived would count a question nobody asked as one nothing \
         could answer"
    );
    assert_eq!(
        settled.decision.by(),
        None,
        "and a decision nobody made names nobody, which the type is what guarantees"
    );
}

#[test]
fn a_fault_every_test_passed_with_is_one_nothing_noticed() {
    let settled = settle(&fault(), &put(&[answered("a", true), answered("b", true)]));
    assert_eq!(
        settled.decision,
        SeamDecision::Unnoticed,
        "the upstream answered 500 and the suite carried on, which is the gap this \
         phase exists to find"
    );
    assert_eq!(settled.decision.by(), None);
}

#[test]
fn a_fault_a_test_failed_with_is_one_the_tests_noticed_and_it_says_which() {
    let settled = settle(
        &fault(),
        &put(&[
            answered("a", true),
            answered("b", false),
            answered("c", false),
        ]),
    );
    assert_eq!(
        settled.decision,
        SeamDecision::Tests {
            noticed_by: "b".to_owned()
        },
        "the first that failed is the one a reader goes to, and naming a later one \
         would send them past the test that already says it. The noticer travels \
         inside the decision, so a run cannot record that the tests noticed and \
         leave nobody named, nor name somebody where nothing noticed"
    );
}

#[test]
fn putting_a_fault_to_one_more_test_never_makes_a_run_look_better() {
    let before = settle(&fault(), &put(&[answered("a", true)]));
    for also in [answered("b", true), answered("b", false)] {
        let after = settle(&fault(), &put(&[answered("a", true), also]));
        assert!(
            after.decision.standing() >= before.decision.standing(),
            "asking one more test can leave a fault where it was or have somebody \
             notice it; a rule that let it come out worse would make measuring more \
             a way of finding less"
        );
    }
}
