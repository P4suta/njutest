// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a recording says stands behind one claim, and the three ways it can answer.

use njutest::report::{Decided, SeamDecision};
use njutest::trace::{
    DischargeRecord, Event, MutantExecRecord, Payload, Read, RouteRecord, WireExchangeRecord,
    WireExecRecord,
};
use njutest::why::{Chain, Claim, Step, Why, why};

/// The identity of the mutation these tests are about.
const MUTANT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The identity of the seam question these tests are about.
const FAULT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// One event, at the sequence given.
fn event(seq: u64, payload: Payload) -> Event {
    Event {
        seq,
        timestamp: "2026-09-18T09:00:00Z".to_owned(),
        elapsed_ms: seq,
        payload,
    }
}

/// A recording of one mutation being routed, put to a target, and noticed.
fn routed_and_killed() -> Vec<Event> {
    vec![
        event(
            1,
            Payload::Route {
                route: RouteRecord {
                    mutant: MUTANT.to_owned(),
                    granularity: rust_mutants::session::Granularity::Block,
                    fallback: None,
                    reaching: vec!["pkg/test/it".to_owned()],
                    tests: Vec::new(),
                    discharged: vec![DischargeRecord {
                        target: "pkg/lib/pkg".to_owned(),
                        proof: "never-infected".to_owned(),
                    }],
                    considered: Vec::new(),
                    reused: None,
                    refused: None,
                    rule: None,
                    carry_refused: None,
                },
            },
        ),
        event(
            2,
            Payload::MutantExec {
                mutant: MutantExecRecord {
                    mutant: MUTANT.to_owned(),
                    target: "pkg/test/it".to_owned(),
                    args: Vec::new(),
                    outcome: "killed".to_owned(),
                    duration_ms: 12,
                    alone: false,
                    step_boundary: None,
                },
            },
        ),
    ]
}

/// A recording of one exchange and the question put about it.
fn observed_and_put() -> Vec<Event> {
    vec![
        event(
            1,
            Payload::WireExchange {
                exchange: WireExchangeRecord {
                    capability: "payments".to_owned(),
                    seq: 0,
                    during: None,
                    duration_ms: 4,
                    read: Read::Http {
                        method: "POST".to_owned(),
                        path: "/orders".to_owned(),
                        status: 201,
                    },
                    request_bytes: 96,
                    response_bytes: 104,
                },
            },
        ),
        event(
            2,
            Payload::WireExec {
                wire: WireExecRecord {
                    fault: FAULT.to_owned(),
                    capability: "payments".to_owned(),
                    seq: 0,
                    rule: njutest::wire::rule::Rule::StatusServerError,
                    decision: SeamDecision::Unnoticed,
                },
            },
        ),
    ]
}

#[test]
fn a_run_that_kept_no_recording_is_not_a_run_that_found_nothing() {
    let asked = Claim::Mutation(MUTANT.to_owned());
    assert_eq!(
        why(&asked, None),
        Why::NotRecorded,
        "a run told to keep no recording establishes nothing about why anything \
         happened, and saying nothing stands behind this would be the command that \
         explains things concluding from how the run was measured"
    );

    let elsewhere = why(&asked, Some(&observed_and_put()));
    assert_eq!(
        elsewhere,
        Why::Unknown { recorded: 0 },
        "and a recording that exists and does not name it is the other answer. One \
         is a missing --trace=, the other is a name typed wrong, and a reader does \
         something different about each"
    );
}

#[test]
fn a_recording_that_does_not_name_a_claim_says_how_many_it_does() {
    let stranger = Claim::Mutation("c".repeat(64));
    assert_eq!(
        why(&stranger, Some(&routed_and_killed())),
        Why::Unknown { recorded: 1 },
        "the count is what lets a page say `this recording names one claim and yours \
         is not one of them` without counting anything itself, which is the whole of \
         the line between the value and the page"
    );
}

#[test]
fn a_mutation_chain_is_what_the_run_did_in_the_order_it_did_it() {
    let asked = Claim::Mutation(MUTANT.to_owned());
    let Why::Followed(Chain::Mutation { id, steps, came_to }) =
        why(&asked, Some(&routed_and_killed()))
    else {
        panic!("a recording that names it answers with the chain");
    };
    assert_eq!(id, MUTANT);
    assert_eq!(
        came_to,
        Decided::Killed {
            by: "pkg/test/it".to_owned()
        }
    );

    let Some(Step::Routed {
        granularity,
        reaching,
        discharged,
        fallback,
    }) = steps.first()
    else {
        panic!("the routing comes first: {steps:?}");
    };
    assert_eq!(*granularity, rust_mutants::session::Granularity::Block);
    assert_eq!(reaching, &["pkg/test/it".to_owned()]);
    assert_eq!(
        discharged,
        &[("pkg/lib/pkg".to_owned(), "never-infected".to_owned())],
        "who was removed and what removed them is the difference between nobody \
         could have noticed and nobody was asked"
    );
    assert_eq!(*fallback, None);

    assert_eq!(
        steps.get(1),
        Some(&Step::Asked {
            target: "pkg/test/it".to_owned(),
            outcome: "killed".to_owned(),
        }),
        "and then it was put to a target, which is the per-mutation execution \
         attribution a report alone cannot answer: {steps:?}"
    );
}

#[test]
fn a_seam_chain_begins_with_the_exchange_that_licensed_the_question() {
    let asked = Claim::Seam(FAULT.to_owned());
    let Why::Followed(Chain::Seam { id, steps, came_to }) = why(&asked, Some(&observed_and_put()))
    else {
        panic!("a recording that names it answers with the chain");
    };
    assert_eq!(id, FAULT);
    assert_eq!(came_to, SeamDecision::Unnoticed);
    assert_eq!(
        steps.first(),
        Some(&Step::Observed {
            capability: "payments".to_owned(),
            seq: 0,
            read: Read::Http {
                method: "POST".to_owned(),
                path: "/orders".to_owned(),
                status: 201,
            },
        }),
        "a catalogue is derived from what went past, so the exchange is where the \
         chain starts; a question with no exchange above it would be one this run \
         invented: {steps:?}"
    );
    assert!(
        matches!(steps.get(1), Some(Step::Put { .. })),
        "and then it was put: {steps:?}"
    );
}

#[test]
fn what_a_claim_came_to_cannot_be_the_other_half_of_the_product() {
    let mutation = why(
        &Claim::Mutation(MUTANT.to_owned()),
        Some(&routed_and_killed()),
    );
    let seam = why(&Claim::Seam(FAULT.to_owned()), Some(&observed_and_put()));
    assert!(
        matches!(mutation, Why::Followed(Chain::Mutation { .. }))
            && matches!(seam, Why::Followed(Chain::Seam { .. })),
        "the claim and its decision are one value rather than two fields, so a \
         mutation coming to a seam's decision is not a thing this can hold"
    );
}
