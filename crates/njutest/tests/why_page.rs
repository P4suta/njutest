// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The page of `njutest why`, which answers one question and must not answer a different one instead.

use njutest::presentation::Terminal;
use njutest::presentation::why::page;
use njutest::report::Decided;
use njutest::why::{Chain, Claim, Step, Why};

fn drawn(claim: &Claim, why: &Why) -> String {
    page(claim, why, Terminal::plain(100))
}

fn mutation(steps: Vec<Step>, came_to: Decided) -> Why {
    Why::Followed(Chain::Mutation {
        id: "a1b2c3".to_owned(),
        steps,
        came_to,
    })
}

#[test]
fn a_run_that_kept_no_recording_is_not_told_as_a_claim_nobody_recorded() {
    let said = drawn(&Claim::Mutation("a1b2c3".to_owned()), &Why::NotRecorded);
    assert!(
        !said.contains("does not name") && !said.contains("no such"),
        "a run that recorded nothing establishes nothing about this claim or any other. \
         Telling the reader their identity is not in the recording is this command \
         concluding from how it was measured, in the command that exists to explain \
         exactly that: {said:?}"
    );
    assert!(
        said.contains("--trace"),
        "what the reader does about it is start the next run with a recording, and a page \
         that diagnoses without saying that makes them go and look it up: {said:?}"
    );
}

#[test]
fn a_claim_a_recording_does_not_hold_says_how_many_it_does() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &Why::Unknown { recorded: 412 },
    );
    assert!(
        said.contains("412"),
        "412 others and nothing typed right is a typo; zero others is a run that recorded \
         nothing of this kind. Without the number the reader cannot tell which, and the \
         two are fixed differently: {said:?}"
    );
}

#[test]
fn a_recording_that_holds_nothing_of_this_kind_still_says_it_recorded() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &Why::Unknown { recorded: 0 },
    );
    let empty = drawn(&Claim::Mutation("a1b2c3".to_owned()), &Why::NotRecorded);
    assert_ne!(
        said, empty,
        "a recording that names no mutation at all and a run with no recording are not the \
         same fact, and the second is not the first with the number zero in it"
    );
}

#[test]
fn what_the_run_established_is_the_first_thing_on_the_page() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &mutation(
            vec![
                Step::ReadBack {
                    run: "2026-09-18T11-02-03Z".to_owned(),
                },
                Step::Asked {
                    target: "core::sign".to_owned(),
                    outcome: "killed".to_owned(),
                },
            ],
            Decided::Killed {
                by: "core::sign".to_owned(),
            },
        ),
    );
    let first = said.lines().take(3).collect::<String>();
    assert!(
        first.contains("core::sign"),
        "the question is why it came out this way, and an answer a reader has to scroll a \
         chain to reach is one they reconstruct themselves: {said:?}"
    );
}

#[test]
fn a_step_that_read_an_answer_back_says_which_run_it_came_from() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &mutation(
            vec![Step::ReadBack {
                run: "2026-09-18T11-02-03Z".to_owned(),
            }],
            Decided::Survived,
        ),
    );
    assert!(
        said.contains("2026-09-18T11-02-03Z"),
        "this run did not establish it, an earlier one did, and a reader who wants to argue \
         with the answer has to be told where it was decided: {said:?}"
    );
}

#[test]
fn a_target_a_proof_removed_is_named_beside_the_proof_that_removed_it() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &mutation(
            vec![Step::Routed {
                granularity: rust_mutants::session::Granularity::Test,
                reaching: vec!["core::sign".to_owned()],
                discharged: vec![("core::abs".to_owned(), "branch".to_owned())],
                fallback: None,
            }],
            Decided::Survived,
        ),
    );
    assert!(
        said.contains("core::abs") && said.contains("branch"),
        "`discharged` with no proof beside it is a verb with no agent: the reader is told \
         something was removed and not what removed it, which is the one thing they would \
         check: {said:?}"
    );
}

#[test]
fn the_page_carries_no_durations_at_all() {
    let said = drawn(
        &Claim::Mutation("a1b2c3".to_owned()),
        &mutation(
            vec![Step::Asked {
                target: "core::sign".to_owned(),
                outcome: "killed".to_owned(),
            }],
            Decided::Survived,
        ),
    );
    assert!(
        !said.contains("ms") && !said.contains(" s ") && !said.contains("took"),
        "why did this come out this way and why did this take four minutes are two \
         questions, and `njutest trace` owns the second. A column that is blank on every \
         chain where each step is instant is a column nobody reads: {said:?}"
    );
}
