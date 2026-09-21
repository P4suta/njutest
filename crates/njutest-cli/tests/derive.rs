// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The faults a recording of a seam licenses, and the ones it does not.

#![expect(
    clippy::panic,
    reason = "a test names what minted identities it refused to"
)]

use njutest_cli::wire::derive::{ID_DOMAIN, derive};
use njutest_cli::wire::rule::Rule;
use njutest_cli::wire::{Exchange, Spoken};

fn derived(observed: &[Exchange]) -> Vec<njutest_cli::wire::derive::Fault> {
    match derive(observed) {
        Ok(faults) => faults,
        Err(error) => panic!("the bounded fixture must mint exact fault identities: {error}"),
    }
}

fn http(seq: u64, path: &str, status: u16) -> Exchange {
    Exchange {
        capability: "api".to_owned(),
        seq,
        during: Some("pkg/test/it".to_owned()),
        duration_ms: 12,
        spoken: Spoken::Http {
            method: "GET".to_owned(),
            path: path.to_owned(),
            status,
            request_bytes: 0,
            response_bytes: 84,
            body_bytes: 40,
            status_line: "HTTP/1.1 200 OK".to_owned(),
        },
    }
}

fn raw(seq: u64) -> Exchange {
    Exchange {
        capability: "db".to_owned(),
        seq,
        during: None,
        duration_ms: 3,
        spoken: Spoken::Raw {
            request_bytes: 40,
            response_bytes: 120,
        },
    }
}

#[test]
fn nothing_is_derived_for_a_seam_nobody_spoke_to() {
    assert!(
        derived(&[]).is_empty(),
        "a catalogue is derived from what a run observed, and a run that observed \
         nothing licenses no question about anything"
    );
}

#[test]
fn every_fault_names_the_exchange_it_was_derived_from() {
    let faults = derived(&[http(0, "/orders", 200)]);
    assert!(
        !faults.is_empty(),
        "an observed exchange licenses questions"
    );
    for fault in &faults {
        assert_eq!(fault.capability, "api");
        assert_eq!(fault.seq, 0);
        assert_eq!(
            fault.during.as_deref(),
            Some("pkg/test/it"),
            "which test was running travels with the fault, because that is what \
             routes it back to the tests that could notice it"
        );
        assert_eq!(fault.id.len(), 64, "an identity is 64 hex characters");
    }
}

#[test]
fn a_protocol_nothing_parsed_licenses_no_question_about_its_answer() {
    let faults = derived(&[raw(0)]);
    let named: Vec<&str> = faults.iter().map(|fault| fault.rule.name()).collect();
    assert!(
        !named.iter().any(|rule| rule.contains("status")),
        "the interposer read nothing of this exchange, so a run that proposed \
         answering it with a 500 would be putting a question to a protocol it \
         cannot speak: {named:?}"
    );
    assert!(
        named.contains(&"truncate-response"),
        "what it can still ask is what needs no parsing — cut it short, hold it \
         up, drop it: {named:?}"
    );
}

#[test]
fn an_answer_that_was_read_licenses_a_question_about_the_answer() {
    let faults = derived(&[http(0, "/orders", 200)]);
    let named: Vec<&str> = faults.iter().map(|fault| fault.rule.name()).collect();
    assert!(
        named.contains(&"status-server-error"),
        "the interposer read a 200, so `what if this had been a 500` is a question \
         about something that happened: {named:?}"
    );
}

#[test]
fn two_seams_that_said_the_same_thing_are_two_different_faults() {
    let one = derived(&[http(0, "/orders", 200)]);
    let mut elsewhere = http(0, "/orders", 200);
    elsewhere.capability = "other".to_owned();
    let two = derived(&[elsewhere]);

    let ids: Vec<&str> = one.iter().map(|fault| fault.id.as_str()).collect();
    for fault in &two {
        assert!(
            !ids.contains(&fault.id.as_str()),
            "a fault is a question about one exchange on one seam, and two seams \
             answering alike are two questions: an identity that ran them together \
             would report one and hide the other"
        );
    }
}

#[test]
fn an_identity_is_a_function_of_the_exchange_and_the_rule_alone() {
    let first = derived(&[http(0, "/orders", 200)]);
    let again = derived(&[http(0, "/orders", 200)]);
    assert_eq!(
        first.iter().map(|one| one.id.clone()).collect::<Vec<_>>(),
        again.iter().map(|one| one.id.clone()).collect::<Vec<_>>(),
        "a later run that observed the same exchange asks the same question under \
         the same name, or nothing it established could be read back"
    );
    assert_eq!(ID_DOMAIN, "njutest-fault-id-v1");
}

#[test]
fn the_identity_of_a_question_is_the_one_the_recipe_states_and_not_this_implementation() {
    let observed = vec![Exchange {
        capability: "api".to_owned(),
        seq: 0,
        during: None,
        duration_ms: 0,
        spoken: Spoken::Http {
            method: "GET".to_owned(),
            path: "/orders".to_owned(),
            status: 200,
            request_bytes: 0,
            response_bytes: 0,
            body_bytes: 0,
            status_line: "HTTP/1.1 200 OK".to_owned(),
        },
    }];
    let minted = derived(&observed);
    let asked = minted
        .iter()
        .find(|one| one.rule == Rule::StatusServerError)
        .expect("the question about the status");
    assert_eq!(
        asked.id, "f714f108a1ce93e4cae5d149115f5f2efc4d4ceb620ccecc762f1c4b914022ed",
        "the audit mints this same identity from the recording with an implementation \
         that never calls this one, and the two agreeing is what makes the audit \
         evidence. Pinning the digest in both places is what keeps that from becoming \
         two copies of one mistake"
    );
    assert!(
        asked.during.is_none(),
        "nothing was running, and the identity does not rest on that in any case"
    );
}

#[test]
fn the_first_exchange_on_a_seam_licenses_no_question_about_what_came_before_it() {
    let observed = vec![http(0, "/orders", 200), http(1, "/orders/1", 200)];
    let minted = derived(&observed);
    let stale: Vec<u64> = minted
        .iter()
        .filter(|one| one.rule == Rule::StaleResponse)
        .map(|one| one.seq)
        .collect();
    assert_eq!(
        stale,
        vec![1],
        "answering an exchange with what the one before it got needs one to have come \
         before it, and proposing it about the first would be a question about a run \
         nobody could put"
    );
}
