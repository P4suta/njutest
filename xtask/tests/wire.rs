// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The audit's own re-derivation of what a seam recording licenses.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest_devkit::result::{ResultState, result_state};
use xtask::wire::{Exchange, Watched, identity, licensed};

/// What the seam reader makes of `recorded`, once it is held to the runner's published schema.
fn read(recorded: &str) -> Result<Watched, xtask::route::ReadError> {
    let checkers = checkers();
    xtask::wire::read(&xtask::route::Checked::read(recorded, &checkers)?)
}

fn returned<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
    assert_eq!(
        result_state(&result),
        ResultState::Returned,
        "the closed fixture was refused: {result:?}"
    );
    match result {
        Ok(value) => Some(value),
        Err(_already_reported) => None,
    }
}

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

fn event(payload: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "seq": 1,
        "timestamp": "2026-09-20T00:00:00Z",
        "elapsed_ms": 0,
        "payload": payload
    })
}

#[test]
fn the_identity_of_a_question_is_the_one_the_recipe_states_and_not_this_implementation() {
    let Some(identity) = returned(identity(&exchange(), "status-server-error")) else {
        return;
    };
    assert_eq!(
        identity, "f714f108a1ce93e4cae5d149115f5f2efc4d4ceb620ccecc762f1c4b914022ed",
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
    let Some(licensed) = returned(licensed(&raw)) else {
        return;
    };
    let rules: Vec<String> = licensed.into_iter().map(|(_, rule)| rule).collect();
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
    let Some(licensed) = returned(licensed(&exchange())) else {
        return;
    };
    assert_eq!(
        licensed.len(),
        6,
        "four that need no reading and two about the status it answered"
    );
}

#[test]
fn a_recording_of_something_else_entirely_holds_no_seam_and_says_so_by_being_empty() {
    let Some(watched) = returned(read(
        &event(&serde_json::json!({"type": "note", "note": {"kind": "other", "detail": "nothing about seams"}})).to_string(),
    )) else {
        return;
    };
    assert!(watched.exchanges.is_empty() && watched.execs.is_empty());
}

#[test]
fn what_the_recording_says_went_past_a_seam_is_read_back_field_for_field() {
    let line = event(&serde_json::json!({
        "type": "wire-exchange",
        "exchange": {
            "capability": "api",
            "seq": 0,
            "during": null,
            "duration_ms": 1,
            "read": { "wire": "http", "method": "GET", "path": "/orders", "status": 200 },
            "request_bytes": 1,
            "response_bytes": 1
        }
    }));
    let Some(watched) = returned(read(&line.to_string())) else {
        return;
    };
    assert_eq!(watched.exchanges, vec![exchange()]);
}

#[test]
fn the_first_exchange_on_a_seam_licenses_no_question_about_what_came_before_it() {
    let Some(first_questions) = returned(licensed(&exchange())) else {
        return;
    };
    let rules: Vec<String> = first_questions.into_iter().map(|(_, rule)| rule).collect();
    assert!(
        !rules.contains(&"stale-response".to_owned()),
        "answering with what the one before it got needs one to have come before, \
         and the first has nothing behind it: {rules:?}"
    );
    let later = Exchange {
        seq: 1,
        ..exchange()
    };
    let Some(later_questions) = returned(licensed(&later)) else {
        return;
    };
    let rules: Vec<String> = later_questions.into_iter().map(|(_, rule)| rule).collect();
    assert!(
        rules.contains(&"stale-response".to_owned()),
        "and the second does: {rules:?}"
    );
}

/// Every published schema, compiled.
fn checkers() -> xtask::schemas::Checkers {
    xtask::schemas::Checkers::compiled().expect("the published schemas compile")
}
