// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phase that puts every fault a recording licensed to the tests, and says what nothing noticed.

use njutest::assure::wire::{Measuring, measure};
use njutest::report::FindingKind;
use njutest::wire::settle::Answered;
use njutest::wire::{Exchange, Spoken};

fn observed() -> Vec<Exchange> {
    vec![Exchange {
        capability: "api".to_owned(),
        seq: 0,
        during: Some("pkg/test/it".to_owned()),
        duration_ms: 5,
        spoken: Spoken::Http {
            method: "GET".to_owned(),
            path: "/orders".to_owned(),
            status: 200,
            request_bytes: 0,
            response_bytes: 12,
            body_bytes: 12,
            status_line: "HTTP/1.1 200 OK".to_owned(),
        },
    }]
}

/// A watch that records nothing and is never cancelled.
macro_rules! watching {
    () => {{
        (
            rust_mutants::runner::Cancel::new(),
            njutest::trace::Recorder::disabled(),
        )
    }};
}

fn answered(target: &str, passed: bool) -> Answered {
    Answered {
        target: target.to_owned(),
        passed,
    }
}

#[test]
fn a_run_that_observed_no_seam_measures_nothing_and_claims_nothing() {
    let held = watching!();
    let done = measure(
        &Measuring { observed: &[] },
        |_fault| njutest::wire::settle::Asked::Answered(vec![answered("a", true)]),
        njutest::watch::Watch::new(&held.0, &held.1),
    )
    .expect("the fault catalogue derives");
    assert!(!done.executed, "there was nothing to put to anything");
    assert!(done.findings.is_empty());
}

#[test]
fn a_fault_the_suite_carried_on_through_is_a_finding_that_says_what_it_asked() {
    let held = watching!();
    let done = measure(
        &Measuring {
            observed: &observed(),
        },
        |_fault| njutest::wire::settle::Asked::Answered(vec![answered("pkg/test/it", true)]),
        njutest::watch::Watch::new(&held.0, &held.1),
    )
    .expect("the fault catalogue derives");
    assert!(done.executed);
    assert!(
        !done.findings.is_empty(),
        "the suite carried on through every question the seam licensed, and each one \
         it carried on through is a gap"
    );
    let first = done.findings.first().expect("a finding");
    assert_eq!(first.kind, FindingKind::WireUnnoticed);
    assert!(
        first.detail.contains("api"),
        "a reader has to know which seam: {}",
        first.detail
    );
    assert!(
        first.detail.contains("answer 500")
            || done
                .findings
                .iter()
                .any(|one| one.detail.contains("answer 500")),
        "and what was asked of it, in the words the catalogue used: {:?}",
        done.findings
            .iter()
            .map(|one| &one.detail)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_fault_the_tests_noticed_is_not_a_finding() {
    let held = watching!();
    let done = measure(
        &Measuring {
            observed: &observed(),
        },
        |_fault| njutest::wire::settle::Asked::Answered(vec![answered("pkg/test/it", false)]),
        njutest::watch::Watch::new(&held.0, &held.1),
    )
    .expect("the fault catalogue derives");
    assert!(
        done.findings.is_empty(),
        "every question was put and every one was answered, which is the phase \
         finding nothing rather than the phase not running: {:?}",
        done.findings
    );
    assert!(done.executed);
}

#[test]
fn a_fault_nothing_ran_is_reported_as_a_hole_rather_than_as_a_survivor() {
    let held = watching!();
    let done = measure(
        &Measuring {
            observed: &observed(),
        },
        |_fault| njutest::wire::settle::Asked::Answered(Vec::new()),
        njutest::watch::Watch::new(&held.0, &held.1),
    )
    .expect("the fault catalogue derives");
    assert!(
        done.findings
            .iter()
            .all(|one| one.kind == FindingKind::NotMeasured),
        "a question nobody was asked is one the run established nothing about, and \
         calling it a survivor would put a gap in the report that no test could \
         ever close: {:?}",
        done.findings
    );
}

#[test]
fn a_run_that_recorded_a_seam_and_put_nothing_says_how_much_it_left_unasked() {
    let stated = njutest::assure::wire::licensing(&observed())
        .expect("the fault catalogue derives")
        .expect("a limitation");
    assert_eq!(stated.name, njutest::assure::wire::NOT_PUT);
    assert!(
        stated.detail.contains("1 exchange") && stated.detail.contains("6 question"),
        "a reader has to be told both how much went past the seam and how many \
         questions that licensed, or the sentence is a shrug: {}",
        stated.detail
    );
}

#[test]
fn a_run_that_watched_a_seam_nothing_went_past_states_no_limitation() {
    assert!(
        njutest::assure::wire::licensing(&[])
            .expect("the fault catalogue derives")
            .is_none(),
        "a seam nothing dialled licensed no question, and saying a run left \
         questions unasked when there were none would send a reader looking for \
         a gap that is not there"
    );
}

#[test]
fn a_question_the_run_put_and_could_not_read_is_not_one_nothing_put() {
    let held = watching!();
    let done = measure(
        &Measuring {
            observed: &observed(),
        },
        |_fault| njutest::wire::settle::Asked::NotMeasured(rust_mutants::outcome::Outcome::Waited),
        njutest::watch::Watch::new(&held.0, &held.1),
    )
    .expect("the fault catalogue derives");
    let named: Vec<&str> = done
        .findings
        .iter()
        .map(|finding| finding.subject.as_str())
        .collect();
    assert!(
        !named.contains(&njutest::assure::wire::NOT_PUT),
        "the exchange did come past and the question was put: an empty list of answers \
         used to mean both that and `the run could not read what the suite did`, and the \
         one sentence both reached told a reader the exchange never came past. {named:?}"
    );
    assert!(
        named.contains(&njutest::assure::wire::NOT_MEASURED),
        "and what it says instead names the run's own silence: {named:?}"
    );
}
