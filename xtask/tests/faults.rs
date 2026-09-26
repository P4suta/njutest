// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the executions a recording holds of one fault say its decision can be.

use xtask::faults::{Control, Evidence, Exec, FaultContradictionError, Site, supports};

fn site(decision: &str, by: Option<&str>) -> Site {
    Site {
        fault: "cccc".to_owned(),
        decision: decision.to_owned(),
        by: by.map(ToOwned::to_owned),
    }
}

fn ran(outcomes: &[(&str, &str)]) -> Vec<Exec> {
    outcomes
        .iter()
        .map(|(target, outcome)| Exec {
            fault: "cccc".to_owned(),
            target: (*target).to_owned(),
            outcome: (*outcome).to_owned(),
            role: "first".to_owned(),
        })
        .collect()
}

fn asked(
    decision: &str,
    by: Option<&str>,
    outcomes: &[(&str, &str)],
) -> Result<(), FaultContradictionError> {
    let execs = ran(outcomes);
    supports(
        &site(decision, by),
        &Evidence {
            execs: execs.iter().collect(),
            ..Evidence::default()
        },
    )
}

/// What a site with nothing run says against a route reaching `reaching`, or a refusal where `reaching` is nothing.
fn routed(decision: &str, reaching: Option<&[String]>) -> Result<(), FaultContradictionError> {
    supports(
        &site(decision, None),
        &Evidence {
            reaching,
            rejected: reaching.is_none(),
            ..Evidence::default()
        },
    )
}

/// A failure on `t`, the control on `t` answering `passed`, and a confirmation coming to `again`.
fn confirmed(passed: bool, again: &str) -> Result<(), FaultContradictionError> {
    let mut execs = ran(&[("t", "killed")]);
    execs.push(Exec {
        fault: "cccc".to_owned(),
        target: "t".to_owned(),
        outcome: again.to_owned(),
        role: "confirmation".to_owned(),
    });
    let control = Control {
        fault: "cccc".to_owned(),
        target: "t".to_owned(),
        passed,
    };
    supports(
        &site("noticed", Some("t")),
        &Evidence {
            execs: execs.iter().collect(),
            controls: vec![&control],
            ..Evidence::default()
        },
    )
}

/// Every decision a fault can be given, against recordings that do and do not support it.
fn cases() -> Vec<(&'static str, Result<(), FaultContradictionError>)> {
    vec![
        (
            "noticed where it failed once and nothing confirmed it",
            asked("noticed", Some("t"), &[("t", "killed")]),
        ),
        (
            "noticed where it failed, the original passed, and it failed again",
            confirmed(true, "killed"),
        ),
        (
            "noticed where the original code failed too",
            confirmed(false, "killed"),
        ),
        (
            "noticed where the failure did not repeat",
            confirmed(true, "survived"),
        ),
        (
            "noticed where it passed",
            asked("noticed", Some("t"), &[("t", "survived"), ("u", "killed")]),
        ),
        (
            "unnoticed where every run passed",
            asked("unnoticed", None, &[("t", "survived")]),
        ),
        ("unnoticed with nothing run", asked("unnoticed", None, &[])),
        (
            "unnoticed beside a run that waited",
            asked("unnoticed", None, &[("t", "survived"), ("u", "waited")]),
        ),
        ("undecided with nothing run", asked("undecided", None, &[])),
        (
            "undecided where a run failed",
            asked("undecided", None, &[("t", "killed")]),
        ),
        (
            "undecided where every run passed",
            asked("undecided", None, &[("t", "survived")]),
        ),
        (
            "unreached with no route recorded",
            asked("unreached", None, &[]),
        ),
        (
            "not-put with no refusal recorded",
            asked("not-put", None, &[]),
        ),
        (
            "unreached where the route reached nothing",
            routed("unreached", Some(&[])),
        ),
        (
            "unreached where the route reached a target",
            routed("unreached", Some(&["t".to_owned()])),
        ),
        ("not-put the compiler refused", routed("not-put", None)),
        (
            "not-put that ran",
            asked("not-put", None, &[("t", "survived")]),
        ),
        (
            "waited where a bound expired",
            asked("waited", None, &[("t", "waited")]),
        ),
        (
            "waited where nothing was bounded",
            asked("waited", None, &[("t", "survived")]),
        ),
        ("a decision no fault has", asked("proved", None, &[])),
    ]
}

#[test]
fn every_decision_is_held_to_the_executions_it_rests_on() {
    let said: Vec<(&str, Option<String>)> = cases()
        .into_iter()
        .map(|(case, answer)| {
            let why = match answer {
                Ok(()) => None,
                Err(why) => Some(format!("{why:?}")),
            };
            (case, why)
        })
        .collect();
    let refused: Vec<&str> = said
        .iter()
        .filter(|(_, why)| why.is_some())
        .map(|(case, _)| *case)
        .collect();
    assert_eq!(
        refused,
        vec![
            "noticed where it failed once and nothing confirmed it",
            "noticed where the original code failed too",
            "noticed where the failure did not repeat",
            "noticed where it passed",
            "unnoticed with nothing run",
            "unnoticed beside a run that waited",
            "undecided where every run passed",
            "unreached with no route recorded",
            "not-put with no refusal recorded",
            "unreached where the route reached a target",
            "not-put that ran",
            "waited where nothing was bounded",
            "a decision no fault has",
        ],
        "{said:#?}"
    );
}
