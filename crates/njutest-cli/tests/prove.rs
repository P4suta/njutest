// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The questions about a seam that need no run at all.

use njutest_cli::wire::derive::derive;
use njutest_cli::wire::prove::{NO_BODY_TO_CUT, discharges};
use njutest_cli::wire::{Exchange, Spoken};

/// One answer of `body_bytes` bytes of body on the `api` seam.
fn answered(body_bytes: u64) -> Exchange {
    Exchange {
        capability: "api".to_owned(),
        seq: 0,
        during: None,
        duration_ms: 1,
        spoken: Spoken::Http {
            method: "DELETE".to_owned(),
            path: "/orders/1".to_owned(),
            status: 204,
            request_bytes: 30,
            response_bytes: 40_u64.saturating_add(body_bytes),
            body_bytes,
        },
    }
}

/// The question of `rule` about the one exchange of `observed`.
fn asking(observed: &[Exchange], rule: &str) -> Option<njutest_cli::wire::derive::Fault> {
    derive(observed).into_iter().find(|one| one.rule == rule)
}

#[test]
fn cutting_an_answer_with_no_body_short_is_a_question_no_observer_could_answer() {
    let observed = vec![answered(0)];
    assert_eq!(
        discharges(
            &asking(&observed, "truncate-response").expect("the question"),
            &observed
        ),
        Some(NO_BODY_TO_CUT),
        "cutting short keeps everything up to and including the blank line that ends \
         the head, so an answer that is all head comes back byte for byte the same. \
         Nothing that reads bytes can tell the two apart, which is a stronger answer \
         than any run could give: not that no test noticed, but that no observer could"
    );
}

#[test]
fn cutting_an_answer_that_has_a_body_short_is_a_question_that_has_to_be_put() {
    let observed = vec![answered(17)];
    assert_eq!(
        discharges(
            &asking(&observed, "truncate-response").expect("the question"),
            &observed
        ),
        None,
        "seventeen bytes the caller had and would not have is a difference somebody \
         could notice, and a proof layer that discharged it would report a gap as \
         assured"
    );
}

#[test]
fn nothing_is_proved_about_a_question_that_changes_what_the_caller_is_handed() {
    let observed = vec![answered(0)];
    for rule in [
        "delay-response",
        "drop-connection",
        "status-server-error",
        "status-not-found",
    ] {
        assert_eq!(
            discharges(&asking(&observed, rule).expect("the question"), &observed),
            None,
            "{rule} changes what the caller is handed or when, and a layer that guessed \
             otherwise would remove a run that was the only thing establishing anything"
        );
    }
}

#[test]
fn nothing_is_proved_about_an_exchange_the_run_did_not_observe() {
    let observed = vec![answered(0)];
    let elsewhere = njutest_cli::wire::derive::Fault {
        capability: "db".to_owned(),
        ..asking(&observed, "truncate-response").expect("the question")
    };
    assert_eq!(
        discharges(&elsewhere, &observed),
        None,
        "the proof rests on what that exchange answered, and a question about a seam \
         this run never watched has nothing under it"
    );
}

#[test]
fn nothing_is_proved_about_a_protocol_the_run_did_not_read() {
    let observed = vec![Exchange {
        spoken: Spoken::Raw {
            request_bytes: 30,
            response_bytes: 40,
        },
        ..answered(0)
    }];
    assert_eq!(
        discharges(
            &asking(&observed, "truncate-response").expect("the question"),
            &observed
        ),
        None,
        "where the head was never found there is no telling how much of the answer \
         was body, and a run that could not tell has established nothing"
    );
}
