// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a route says about a mutation, read as a pure function of the record.

/// What one target's tests reached, as a table a case reads at a glance.
type Reached<'a> = &'a [(&'a str, &'a [u32])];

/// Every target of a record, each with what its tests reached.
type Records<'a> = &'a [(&'a str, Reached<'a>)];

use rust_mutants::count::{Count, Tests};
use rust_mutants::session::{Fallback, Granularity, Route, RouteAccountingError, Routing};
use rust_mutants::touch::{Seen, TargetTouches, Touched};

/// A record in which each named target's tests reached exactly the mutations given.
fn touched(targets: Records<'_>, compared: &[u32]) -> Touched {
    let mut held = Touched::default();
    held.narrowing.compared = compared.iter().copied().collect();
    for (target, tests) in targets {
        let mut reached = Seen::default();
        for (test, indices) in *tests {
            reached
                .tests
                .entry((*test).to_owned())
                .or_insert_with(|| indices.iter().copied().collect());
        }
        let mut touches = TargetTouches::default();
        touches.reached = reached;
        touches.ran = tests.iter().map(|(test, _)| (*test).to_owned()).collect();
        held.targets.entry((*target).to_owned()).or_insert(touches);
    }
    held
}

#[test]
fn a_route_that_reaches_nothing_names_every_target_it_asked() {
    let record = touched(
        &[
            ("demo/lib/demo", &[("one", &[7])]),
            ("demo/test/wide", &[("two", &[7])]),
        ],
        &[],
    );
    let targets = ["demo/lib/demo", "demo/test/wide"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(
        route.granularity(),
        Granularity::Unreached,
        "nothing of either target reached mutation 1"
    );
    assert_eq!(
        route.considered(),
        targets,
        "and a layer that removed every execution has to say who it asked, or the word \
         `unreached` has nothing behind it and an audit can only confirm that the engine said it"
    );
}

#[test]
fn a_route_that_keeps_a_target_names_nobody_as_having_missed_it() {
    let record = touched(&[("demo/lib/demo", &[("one", &[1])])], &[]);
    let targets = ["demo/lib/demo"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(route.reaching(), ["demo/lib/demo"]);
    assert!(
        route.considered().is_empty(),
        "one route cannot answer one question two ways: a target it kept is not one it says \
         reached nothing"
    );
}

#[test]
fn a_route_every_target_answered_says_nothing_widened_it() {
    let record = touched(&[("demo/lib/demo", &[("one", &[1])])], &[]);
    let targets = ["demo/lib/demo"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.fallback(),
        None,
        "every target of this record was asked and answered, so nothing was widened back"
    );

    let unmeasured = Touched::default();
    let widened = Route::by_touch(
        &unmeasured,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );
    assert_eq!(
        widened.fallback(),
        Some(Fallback::NotMeasured),
        "and a record that measured nothing says so rather than removing anything"
    );
}

#[test]
fn a_target_the_record_does_not_name_is_kept_and_says_the_route_is_incomplete() {
    let record = touched(&[("demo/lib/demo", &[("one", &[9])])], &[]);
    let targets = ["demo/lib/demo", "demo/test/absent"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(
        route.reaching(),
        ["demo/test/absent"],
        "a target nothing was recorded about is one nothing was established about, and it stays"
    );
    assert_eq!(
        route.fallback(),
        Some(Fallback::TouchIncomplete),
        "and the route says why it is wider than the record alone would make it"
    );
    assert!(
        route.considered().is_empty(),
        "a route that keeps something carries no such list: the target the record placed \
         elsewhere is dropped without being named, which is the asymmetry `considered` closes \
         only for a route that keeps nothing"
    );
}

/// A route over three targets that all reached the mutation, in the order they were offered.
fn three() -> Route {
    let record = touched(
        &[
            ("demo/lib/demo", &[("one", &[1])]),
            ("demo/test/wide", &[("two", &[1])]),
            ("demo/test/last", &[("three", &[1])]),
        ],
        &[],
    );
    let targets = ["demo/lib/demo", "demo/test/wide", "demo/test/last"];
    Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    )
}

#[test]
fn a_route_nothing_answered_ran_nothing() {
    assert!(
        three().executed("", false).is_empty(),
        "a run that has no answer started no process, and a record saying otherwise would put \
         work in the ledger nobody did"
    );
}

#[test]
fn a_detected_answer_ran_the_targets_up_to_the_one_that_detected() {
    assert_eq!(
        three().executed("demo/test/wide", true),
        ["demo/lib/demo", "demo/test/wide"],
        "a run walks the route in order and stops at the first target that detects, so the one \
         after it was never started"
    );
    assert_eq!(
        three().executed("demo/lib/demo", true),
        ["demo/lib/demo"],
        "and the first target detecting is the whole of what ran"
    );
}

#[test]
fn an_answer_nothing_detected_ran_every_target_of_the_route() {
    assert_eq!(
        three().executed("demo/test/last", false),
        ["demo/lib/demo", "demo/test/wide", "demo/test/last"],
        "nothing detected, so nothing stopped the walk and every target of the route was asked"
    );
}

#[test]
fn an_answer_the_route_does_not_hold_is_one_target_on_its_own() {
    assert_eq!(
        three().executed("demo/test/elsewhere", true),
        ["demo/test/elsewhere"],
        "`--target` asks one target the route never named, and what ran is that target and \
         nothing else"
    );
}

/// A route over two targets, the first narrowed to one of its tests and the second not.
fn narrowed() -> Route {
    let record = touched(
        &[
            ("demo/lib/demo", &[("one", &[1]), ("two", &[9])]),
            ("demo/test/wide", &[("three", &[1]), ("four", &[1])]),
        ],
        &[],
    );
    let targets = ["demo/lib/demo", "demo/test/wide"];
    Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    )
}

#[test]
fn a_route_narrowed_to_some_tests_says_test_rather_than_block() {
    assert_eq!(
        narrowed().granularity(),
        Granularity::Test,
        "a route that put the mutation to some of a target's tests and not all of them is a \
         narrower question than the block it sits in, and a reader counting work has to see that"
    );
    assert_eq!(
        narrowed()
            .tests()
            .get("demo/lib/demo")
            .map(Vec::as_slice)
            .unwrap_or_default(),
        ["one"],
        "and it names exactly the tests, because those are what the process is asked for"
    );
    assert!(
        !narrowed().tests().contains_key("demo/test/wide"),
        "a target it did not narrow is named nowhere here: every test of it runs, and a list \
         of them would be a second way to say the same thing"
    );
}

#[test]
fn what_a_route_starts_is_the_tests_it_named_and_every_test_of_what_it_did_not_narrow() {
    assert_eq!(
        narrowed().started(|_target| 100),
        Ok(Count::<Tests>::new(101)),
        "one named test of the first target, and all hundred of the second, which is the work \
         the route asks for rather than the work a whole run would"
    );
}

#[test]
fn a_narrowed_route_refuses_an_empty_or_impossible_test_set() {
    let empty = Route::Block {
        reaching: vec![rust_mutants::session::Reaches {
            target: "demo/lib/demo".to_owned(),
            tests: rust_mutants::session::Asked::These(Vec::new()),
        }],
        discharged: Vec::new(),
        fallback: None,
    };
    assert_eq!(
        empty.started(|_target| 2),
        Err(RouteAccountingError::EmptyNamedTestSet),
        "retaining a target while naming no test is not zero work; it is an invalid route"
    );

    let too_many = narrowed().started(|_target| 0);
    assert_eq!(
        too_many,
        Err(RouteAccountingError::NamedTestsExceedBaseline {
            named: 1,
            measured: 0,
        }),
        "a named test cannot be charged against a target said to contain no tests"
    );
}

#[test]
fn what_a_route_costs_is_the_share_of_each_baseline_its_tests_come_to() {
    use std::time::Duration;
    let cost = narrowed()
        .costing(|_target| rust_mutants::session::Timing::new(Duration::from_millis(100), 10));
    assert_eq!(
        cost,
        Ok(Duration::from_millis(110)),
        "a target narrowed to one of ten tests costs a tenth of its baseline, and one that was \
         not narrowed costs the whole of it; pricing every target at its whole baseline is what \
         an estimate that knows one number reaches for"
    );
}

#[test]
fn a_narrowed_target_cannot_name_tests_a_baseline_says_never_ran() {
    use std::time::Duration;
    let cost = narrowed()
        .costing(|_target| rust_mutants::session::Timing::new(Duration::from_millis(60), 0));
    assert_eq!(
        cost,
        Err(RouteAccountingError::NamedTestsExceedBaseline {
            named: 1,
            measured: 0,
        }),
        "a route cannot silently price a named test against a baseline that says no test ran"
    );
}

#[test]
fn an_unnarrowed_zero_test_target_still_costs_its_process_baseline() {
    use std::time::Duration;
    let route = Route::All {
        reaching: vec!["demo/lib/demo".to_owned()],
        fallback: Fallback::NotMeasured,
    };
    assert_eq!(
        route.costing(|_target| {
            rust_mutants::session::Timing::new(Duration::from_millis(60), 0)
        }),
        Ok(Duration::from_millis(60)),
        "an unmeasured target still starts one process even when that process reports no tests"
    );
}

#[test]
fn a_route_every_proof_removed_says_so_and_widens_nothing() {
    let route = Route::Discharged {
        discharged: vec![rust_mutants::session::Discharge {
            target: "demo/lib/demo".to_owned(),
            proof: rust_mutants::session::NEVER_INFECTED,
        }],
    };
    assert_eq!(
        route.granularity(),
        Granularity::Discharged,
        "a mutation every target was proved unable to notice is not one nothing reached, and a \
         reader deciding whether the tests have a gap there needs the two apart"
    );
    assert_eq!(
        route.fallback(),
        None,
        "nothing widened it: a proof removed each target, and a fallback would say the \
         measurement failed to place one"
    );
    assert!(
        route.reaching().is_empty() && route.considered().is_empty(),
        "and it keeps nobody, so there is nothing for it to name but the proofs"
    );
}

#[test]
fn what_a_route_narrows_an_execution_to_is_one_ordered_list_of_names() {
    let route = Route::Block {
        reaching: vec![
            rust_mutants::session::Reaches {
                target: "demo/test/wide".to_owned(),
                tests: rust_mutants::session::Asked::Every,
            },
            rust_mutants::session::Reaches {
                target: "demo/lib/demo".to_owned(),
                tests: rust_mutants::session::Asked::Every,
            },
        ],
        discharged: vec![rust_mutants::session::Discharge {
            target: "demo/lib/demo".to_owned(),
            proof: rust_mutants::session::NEVER_INFECTED,
        }],
        fallback: None,
    };
    assert_eq!(
        route.narrowing().unwrap_or_default(),
        ["demo/lib/demo", "demo/test/wide"],
        "this is the one place a narrowing is decided, and a caller with its own evidence runs \
         exactly what it names: a target said twice is a second process for an answer already \
         had, and an order that follows how the targets happened to be offered makes two runs \
         of one tree ask two different questions"
    );
}
