// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Answer, Observed, Wait, agrees, answer};

fn by_the_table(wait: Wait, named_a_failure: bool) -> Answer {
    match (wait, named_a_failure) {
        (Wait::Exited | Wait::Answered, true) => Answer::Answered,
        (Wait::Answered, false) => Answer::Contradicted,
        (Wait::Exited | Wait::Other, false) | (Wait::Other, true) => Answer::Unanswered,
    }
}

#[test]
fn every_ending_comes_to_what_the_table_says() {
    for wait in Wait::ALL {
        for named_a_failure in [false, true] {
            assert_eq!(
                answer(wait, named_a_failure),
                by_the_table(wait, named_a_failure),
                "{wait:?}, a failure named: {named_a_failure}"
            );
        }
    }
}

#[test]
fn a_named_failure_is_answered_whichever_ending_arrives_first() {
    assert_eq!(answer(Wait::Exited, true), answer(Wait::Answered, true));
    assert_eq!(answer(Wait::Exited, true), Answer::Answered);
}

#[test]
fn the_check_accepts_exactly_the_ending_the_table_gives() {
    for wait in Wait::ALL {
        for named_a_failure in [false, true] {
            for capture_failed in [false, true] {
                for reported_answered in [false, true] {
                    let ruled = by_the_table(wait, named_a_failure);
                    let expected = ruled != Answer::Contradicted
                        && reported_answered == (ruled == Answer::Answered && !capture_failed);
                    let observed = Observed {
                        wait,
                        named_a_failure,
                        capture_failed,
                    };
                    assert_eq!(
                        agrees(observed, reported_answered),
                        expected,
                        "{wait:?}, a failure named: {named_a_failure}, the capture failed: \
                         {capture_failed}, reported answered: {reported_answered}"
                    );
                }
            }
        }
    }
}
