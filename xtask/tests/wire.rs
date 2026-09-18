// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The audit's own re-derivation of what a seam recording licenses.

use xtask::wire::{Exchange, identity, licensed, read};

/// One `GET /orders` that was answered 200 on the `api` seam.
fn exchange() -> Exchange {
    Exchange {
        capability: "api".to_owned(),
        seq: 0,
        wire: "http".to_owned(),
        method: Some("GET".to_owned()),
        path: Some("/orders".to_owned()),
        status: Some(200),
    }
}

#[test]
fn the_identity_of_a_question_is_the_one_the_recipe_states_and_not_this_implementation() {
    assert_eq!(
        identity(&exchange(), "status-server-error"),
        "f714f108a1ce93e4cae5d149115f5f2efc4d4ceb620ccecc762f1c4b914022ed",
        "a fault identity is minted from the exchange and the rule alone, by a recipe \
         two implementations follow separately. Pinning it here is what makes the \
         runner and this audit agreeing mean something: if only one of them changed, \
         one of these tests fails rather than both quietly moving together"
    );
}

#[test]
fn an_exchange_nobody_read_licenses_only_the_questions_that_need_no_reading() {
    let raw = Exchange {
        wire: "raw".to_owned(),
        method: None,
        path: None,
        status: None,
        ..exchange()
    };
    let rules: Vec<String> = licensed(&raw).into_iter().map(|(_, rule)| rule).collect();
    assert_eq!(
        rules,
        vec![
            "truncate-response".to_owned(),
            "delay-response".to_owned(),
            "drop-connection".to_owned(),
            "replay-request".to_owned()
        ],
        "cutting short, holding up, dropping and delivering twice need no reading; \
         asking a seam to answer 500 where nothing parsed a status is a question \
         about a program this run never saw"
    );
}

#[test]
fn an_exchange_the_run_read_licenses_the_questions_about_what_it_said_as_well() {
    assert_eq!(
        licensed(&exchange()).len(),
        6,
        "four that need no reading and two about the status it answered"
    );
}

#[test]
fn a_recording_of_something_else_entirely_holds_no_seam_and_says_so_by_being_empty() {
    let watched = read(&serde_json::json!({"type": "note"}).to_string());
    assert!(watched.exchanges.is_empty() && watched.execs.is_empty());
}

#[test]
fn what_the_recording_says_went_past_a_seam_is_read_back_field_for_field() {
    let line = serde_json::json!({
        "type": "wire-exchange",
        "exchange": {
            "capability": "api",
            "seq": 0,
            "wire": "http",
            "method": "GET",
            "path": "/orders",
            "status": 200
        }
    });
    assert_eq!(read(&line.to_string()).exchanges, vec![exchange()]);
}

#[test]
fn the_first_exchange_on_a_seam_licenses_no_question_about_what_came_before_it() {
    let rules: Vec<String> = licensed(&exchange())
        .into_iter()
        .map(|(_, rule)| rule)
        .collect();
    assert!(
        !rules.contains(&"stale-response".to_owned()),
        "answering with what the one before it got needs one to have come before, \
         and the first has nothing behind it: {rules:?}"
    );
    let later = Exchange {
        seq: 1,
        ..exchange()
    };
    let rules: Vec<String> = licensed(&later).into_iter().map(|(_, rule)| rule).collect();
    assert!(
        rules.contains(&"stale-response".to_owned()),
        "and the second does: {rules:?}"
    );
}
