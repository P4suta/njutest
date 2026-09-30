// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crash's decision is re-derived from its ordered steps exactly, so a report cannot claim more, or less, than they show.

use xtask::crashes::{Asked, Crashed, Issued, Run, Site, Step, Unmade, decided, disagreements};

fn run(test: &str, stage: &str, ended: &str, files: &[&str]) -> Step {
    ran(record(test, stage, ended, files))
}

/// The run `run` records, as a record rather than a step.
fn record(test: &str, stage: &str, ended: &str, files: &[&str]) -> Run {
    let (exit_code, outcome) = match ended {
        "stopped" | "chose" => (93, "killed"),
        "passed" => (0, "survived"),
        _ => (101, "killed"),
    };
    let files: Vec<String> = files.iter().map(|one| (*one).to_owned()).collect();
    let (left, failed) = if stage == "crash" {
        (files, Vec::new())
    } else {
        (Vec::new(), files)
    };
    Run {
        target: "pkg/test/it".to_owned(),
        test: test.to_owned(),
        stage: stage.to_owned(),
        sealed: false,
        exit_code: Some(exit_code),
        outcome: outcome.to_owned(),
        noticed: ended == "stopped",
        issued: (stage == "crash").then(|| {
            let issued = Issued {
                mutant: "d".repeat(64),
                catalog: "c".repeat(64),
                nonce: "0".repeat(32),
                read: None,
            };
            let read = (ended == "stopped").then(|| {
                format!(
                    "{}\t{}\t{}\t{}\n",
                    xtask::crashes::NOTICE_SCHEMA,
                    issued.nonce,
                    issued.catalog,
                    issued.mutant
                )
            });
            Issued { read, ..issued }
        }),
        left,
        unnamed: None,
        failed,
    }
}

/// `run` as the step a recording holds it as.
fn ran(run: Run) -> Step {
    Step::Ran(Box::new(run))
}

fn route(asked: &[(&str, Option<&[&str]>)]) -> Step {
    Step::Route(
        asked
            .iter()
            .map(|(target, tests)| Asked {
                target: (*target).to_owned(),
                tests: tests.map(|tests| tests.iter().map(|one| (*one).to_owned()).collect()),
            })
            .collect(),
    )
}

fn asks_t() -> Step {
    route(&[("pkg/test/it", Some(&["t"]))])
}

fn decision(steps: &[Step]) -> Result<(String, String), Unmade> {
    decided("dddd", &steps.iter().collect::<Vec<_>>(), false)
        .map(|decided| (decided.site.decision, decided.site.on))
}

/// A sealed instance of `test` at `stage` that came to `outcome`, leaving `left` where it is a stop and failing `failed` where it is a next run.
fn sealed(test: &str, stage: &str, outcome: &str, (left, failed): (&[&str], &[&str])) -> Step {
    let native = record(
        test,
        stage,
        if outcome == "halted" {
            "stopped"
        } else {
            "passed"
        },
        &[],
    );
    ran(Run {
        sealed: true,
        exit_code: None,
        outcome: outcome.to_owned(),
        left: left.iter().map(|one| (*one).to_owned()).collect(),
        failed: failed.iter().map(|one| (*one).to_owned()).collect(),
        ..native
    })
}

#[test]
fn a_sealed_crash_is_decided_in_one_round_and_says_it_was_sealed() {
    let one = |steps: &[Step]| {
        decided("dddd", &steps.iter().collect::<Vec<_>>(), false)
            .map(|decided| (decided.site.decision, decided.site.sealed))
    };
    let halted = sealed("t", "crash", "halted", (&["count"], &[]));
    assert_eq!(
        one(&[
            asks_t(),
            halted.clone(),
            sealed("t", "next", "passed", (&[], &[]))
        ]),
        Ok(("restarted".to_owned(), true)),
        "a next instance that passed over what the halt left restarts it"
    );
    assert_eq!(
        one(&[
            asks_t(),
            halted.clone(),
            sealed("t", "next", "panicked", (&[], &["t"]))
        ]),
        Ok(("corrupt".to_owned(), true)),
        "a next instance that detected over what the halt left is corrupt, with no fresh run \
         and no second stop, since the same instance comes out the same every time"
    );
    assert_eq!(
        one(&[
            asks_t(),
            halted,
            sealed("t", "next", "exited-early", (&[], &[]))
        ]),
        Ok(("undecided".to_owned(), true)),
        "a next instance that established nothing decides nothing"
    );
    assert_eq!(
        one(&[asks_t(), sealed("t", "crash", "halted", (&[], &[]))]),
        Ok(("unshared".to_owned(), true)),
        "a halt that left nothing leaves the next instance nothing to read"
    );
    assert_eq!(
        one(&[
            route(&[("pkg/test/it", Some(&["t", "u"]))]),
            sealed("t", "crash", "passed", (&[], &[])),
            run("u", "crash", "stopped", &["count"]),
            run("u", "next", "passed", &[]),
        ]),
        Ok(("restarted".to_owned(), false)),
        "a decision that rests on a native run as well is not a sealed one"
    );
}

#[test]
fn a_sealed_next_instance_that_could_not_start_over_the_stop_decides_nothing() {
    let halted = sealed("t", "crash", "halted", (&["count"], &[]));
    assert_eq!(
        decision(&[
            asks_t(),
            halted,
            sealed("t", "next", "unstartable", (&[], &[]))
        ]),
        Ok(("undecided".to_owned(), "pkg/test/it::t".to_owned())),
        "what the stop left is no state an instance of the test starts in, which says nothing \
         of whether the program could start over it"
    );
    assert!(
        decision(&[asks_t(), sealed("t", "crash", "unstartable", (&[], &[]))]).is_err(),
        "only a next instance can fail to start over a stop"
    );
}

#[test]
fn a_sealed_sequence_no_run_makes_is_refused() {
    let halted = sealed("t", "crash", "halted", (&["count"], &[]));
    let cases: Vec<(&str, Vec<Step>)> = vec![
        (
            "a native next run after a sealed halt",
            vec![asks_t(), halted.clone(), run("t", "next", "passed", &[])],
        ),
        (
            "a sealed next run after a native stop",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                sealed("t", "next", "passed", (&[], &[])),
            ],
        ),
        (
            "a sealed next run that detected and names no failure",
            vec![
                asks_t(),
                halted.clone(),
                sealed("t", "next", "failed", (&[], &[])),
            ],
        ),
        (
            "a sealed next run that passed and names a failure",
            vec![
                asks_t(),
                halted.clone(),
                sealed("t", "next", "passed", (&[], &["t"])),
            ],
        ),
        (
            "a sealed next run that halted",
            vec![
                asks_t(),
                halted.clone(),
                sealed("t", "next", "halted", (&[], &[])),
            ],
        ),
        (
            "a sealed run with an exit status",
            vec![
                asks_t(),
                match sealed("t", "crash", "halted", (&["count"], &[])) {
                    Step::Ran(run) => ran(Run {
                        exit_code: Some(93),
                        ..*run
                    }),
                    other => other,
                },
            ],
        ),
        (
            "a corrupt sealed stop confirmed with a fresh run",
            vec![
                asks_t(),
                halted,
                sealed("t", "next", "panicked", (&[], &["t"])),
                sealed("t", "fresh", "passed", (&[], &[])),
            ],
        ),
    ];
    for (case, steps) in cases {
        assert!(decision(&steps).is_err(), "{case} is refused");
    }
}

fn corrupted(again: &str) -> Vec<Step> {
    let mut steps = vec![
        asks_t(),
        run("t", "crash", "stopped", &["count"]),
        run("t", "next", "failed", &["t"]),
    ];
    for round in 1..=xtask::crashes::CONFIRMATIONS {
        let failed = if round == xtask::crashes::CONFIRMATIONS {
            again
        } else {
            "t"
        };
        steps.extend([
            run("t", "fresh", "passed", &[]),
            run("t", "crash", "stopped", &["count"]),
            run("t", "next", "failed", &[failed]),
        ]);
    }
    steps
}

#[test]
fn each_sequence_a_run_makes_decides_exactly_one_thing() {
    let on = "pkg/test/it::t".to_owned();
    let cases = [
        (
            "a stop the next run passed over",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                run("t", "next", "passed", &[]),
            ],
            ("restarted", on.as_str()),
        ),
        (
            "a failure reproduced",
            corrupted("t"),
            ("corrupt", on.as_str()),
        ),
        (
            "a second next run that failed another test",
            corrupted("u"),
            ("undecided", on.as_str()),
        ),
        (
            "a stop that left nothing",
            vec![asks_t(), run("t", "crash", "stopped", &[])],
            ("unshared", on.as_str()),
        ),
        (
            "a test that passed without stopping",
            vec![asks_t(), run("t", "crash", "passed", &[])],
            ("unreached", ""),
        ),
        (
            "a target whose tests are not known",
            vec![
                route(&[("pkg/test/other", None), ("pkg/test/it", Some(&["t"]))]),
                run("t", "crash", "passed", &[]),
            ],
            ("undecided", "pkg/test/other"),
        ),
        (
            "the stop's status with no notice the runtime made it",
            vec![asks_t(), run("t", "crash", "chose", &["count"])],
            ("undecided", on.as_str()),
        ),
        (
            "a failure that did not reproduce in a later round",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                run("t", "next", "failed", &["t"]),
                run("t", "fresh", "passed", &[]),
                run("t", "crash", "stopped", &["count"]),
                run("t", "next", "failed", &["t"]),
                run("t", "fresh", "failed", &["t"]),
            ],
            ("undecided", on.as_str()),
        ),
        ("a refusal", vec![Step::Rejected], ("not-put", "")),
        (
            "a stop that wrote into the tree",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                run("t", "next", "passed", &[]),
                Step::Outside,
            ],
            ("undecided", "crash-after-write"),
        ),
    ];
    for (case, steps, (expected, on)) in cases {
        assert_eq!(
            decision(&steps),
            Ok((expected.to_owned(), on.to_owned())),
            "{case}"
        );
    }
}

#[test]
fn a_sequence_no_run_makes_is_refused_rather_than_read_around() {
    let cases = [
        ("no route", vec![run("t", "crash", "stopped", &["count"])]),
        ("a route whose test was never run", vec![asks_t()]),
        (
            "a corrupt stop confirmed fewer times than the runner confirms it",
            corrupted("t").into_iter().take(6).collect(),
        ),
        (
            "a next run of another test",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                run("u", "next", "passed", &[]),
            ],
        ),
        (
            "a run after the decision",
            vec![
                asks_t(),
                run("t", "crash", "stopped", &["count"]),
                run("t", "next", "passed", &[]),
                run("t", "crash", "stopped", &["count"]),
            ],
        ),
        (
            "a refusal that ran",
            vec![Step::Rejected, run("t", "crash", "passed", &[])],
        ),
        ("an unstained crash left alone", vec![Step::Tainted]),
        (
            "a step this audit cannot read",
            vec![Step::Unread("later".to_owned())],
        ),
    ];
    for (case, steps) in cases {
        assert!(decision(&steps).is_err(), "{case} is refused");
    }
}

#[test]
fn a_report_that_drops_renames_or_softens_a_crash_disagrees_with_its_recording() {
    let recorded = Crashed {
        steps: corrupted("t")
            .into_iter()
            .map(|step| ("dddd".to_owned(), step))
            .collect(),
    };
    let corrupt = Site {
        crash: "dddd".to_owned(),
        decision: "corrupt".to_owned(),
        on: "pkg/test/it::t".to_owned(),
        left: Vec::new(),
        failed: vec!["t".to_owned()],
        sealed: false,
    };
    assert!(
        disagreements(std::slice::from_ref(&corrupt), &recorded).is_empty(),
        "the truth holds"
    );
    let lies = [
        ("dropped", Vec::new()),
        (
            "softened to undecided",
            vec![Site {
                decision: "undecided".to_owned(),
                failed: Vec::new(),
                ..corrupt.clone()
            }],
        ),
        (
            "renamed and unreached",
            vec![Site {
                crash: "eeee".to_owned(),
                decision: "unreached".to_owned(),
                on: String::new(),
                failed: Vec::new(),
                ..corrupt
            }],
        ),
    ];
    for (case, reported) in lies {
        assert!(
            !disagreements(&reported, &recorded).is_empty(),
            "{case} is refused"
        );
    }
}

#[test]
fn a_stop_in_a_child_the_test_started_leaves_the_crash_undecided_whatever_the_parent_did() {
    let stopped = run("t", "crash", "stopped", &[]);
    let Step::Ran(stopped) = stopped else {
        panic!("a run is a run");
    };
    let tolerated = ran(Run {
        exit_code: Some(0),
        outcome: "survived".to_owned(),
        noticed: false,
        left: Vec::new(),
        ..*stopped
    });
    assert_eq!(
        decision(&[asks_t(), tolerated]),
        Ok(("undecided".to_owned(), "pkg/test/it::t".to_owned())),
        "the runtime published this run's notice, so a process the test started stopped at the \
         call; the test's own process did not, and a parent that tolerated its child's status is \
         not a test that passed without reaching the call"
    );
}

#[test]
fn a_stop_that_left_an_entry_whose_name_is_not_text_is_undecided_and_only_a_stop_names_one() {
    let Step::Ran(stopped) = run("t", "crash", "stopped", &[]) else {
        panic!("a run is a run");
    };
    let unnamed = Run {
        unnamed: Some("bytes:746f726e2dff".to_owned()),
        ..*stopped
    };
    assert_eq!(
        decision(&[asks_t(), ran(unnamed.clone())]),
        Ok(("undecided".to_owned(), "pkg/test/it::t".to_owned())),
        "what the stop left cannot be named, so no next run was asked and nothing is said of \
         whether it could start over it"
    );
    assert!(
        decision(&[
            asks_t(),
            ran(Run {
                left: vec!["count".to_owned()],
                ..unnamed.clone()
            })
        ])
        .is_err(),
        "a stop that named what it left and an entry it could not is a record no run makes"
    );
    let Step::Ran(passed) = run("t", "crash", "passed", &[]) else {
        panic!("a run is a run");
    };
    assert!(
        decision(&[
            asks_t(),
            ran(Run {
                unnamed: unnamed.unnamed,
                ..*passed
            })
        ])
        .is_err(),
        "a run that did not stop left nothing, named or not"
    );
}

#[test]
fn a_nonce_is_one_run_s_and_a_second_run_carrying_it_is_refused() {
    let ids = std::collections::BTreeMap::from([
        ("dddd".to_owned(), "d".repeat(64)),
        ("eeee".to_owned(), "d".repeat(64)),
    ]);
    let one = Crashed {
        steps: vec![("dddd".to_owned(), run("t", "crash", "stopped", &["a"]))],
    };
    assert!(
        xtask::crashes::issued_disagreements(&ids, &one).is_empty(),
        "one run carrying its own nonce is what the engine issues"
    );
    let twice = Crashed {
        steps: vec![
            ("dddd".to_owned(), run("t", "crash", "stopped", &["a"])),
            ("eeee".to_owned(), run("t", "crash", "stopped", &["a"])),
        ],
    };
    let said = xtask::crashes::issued_disagreements(&ids, &twice);
    assert!(
        said.iter().any(|(crash, why)| crash == "eeee"
            && why.contains("carries the nonce already issued a run of dddd")),
        "a nonce issued to one run and read back from another is a notice one run could have \
         written for the other, so the second is refused: {said:?}"
    );
    let other = std::collections::BTreeMap::from([("dddd".to_owned(), "e".repeat(64))]);
    assert!(
        xtask::crashes::issued_disagreements(&other, &one)
            .iter()
            .any(|(_crash, why)| why.contains("another mutation")),
        "a run issued another mutation than the report's site names is refused too"
    );
}

#[test]
fn once_a_stop_wrote_into_the_tree_every_later_crash_is_left_alone() {
    let steps = |later: Vec<Step>| Crashed {
        steps: [
            asks_t(),
            run("t", "crash", "stopped", &["count"]),
            run("t", "next", "passed", &[]),
            Step::Outside,
        ]
        .into_iter()
        .map(|step| ("dddd".to_owned(), step))
        .chain(later.into_iter().map(|step| ("eeee".to_owned(), step)))
        .collect(),
    };
    let undecided = |crash: &str| Site {
        crash: crash.to_owned(),
        decision: "undecided".to_owned(),
        on: "crash-after-write".to_owned(),
        ..Site::default()
    };
    let reported = [undecided("dddd"), undecided("eeee")];
    assert!(
        disagreements(&reported, &steps(vec![Step::Tainted])).is_empty(),
        "a later crash left alone holds"
    );
    assert!(
        !disagreements(
            &reported,
            &steps(vec![asks_t(), run("t", "crash", "passed", &[])])
        )
        .is_empty(),
        "a later crash that ran over the written tree is refused"
    );
}

#[test]
fn a_recorded_run_that_carries_no_issue_is_read_as_one_that_was_not_crashed() {
    let line = |issued: &str| {
        format!(
            "{{\"seq\":1,\"timestamp\":\"2026-09-06T00:00:00Z\",\"elapsed_ms\":0,\"payload\":{{\"type\":\"crash-exec\",\"crash\":{{\"crash\":\"dddddddddddddddddddd\",\"target\":\"pkg/test/it\",\"test\":\"t\",\"stage\":\"next\",\"sealed\":false,\"exit_code\":0,\"outcome\":\"survived\",\"noticed\":false,\"issued\":{issued},\"left\":[],\"unnamed\":null,\"failed\":[]}}}}}}\n"
        )
    };
    let checkers = xtask::schemas::Checkers::compiled().expect("the published schemas compile");
    let recorded = |text: &str| {
        xtask::route::Checked::read(text, &checkers).map(|checked| xtask::crashes::read(&checked))
    };
    let read = |issued: &str| recorded(&line(issued)).expect("a recording this audit can read");
    assert!(
        matches!(
            read("null").steps.as_slice(),
            [(_, Step::Ran(run))] if run.issued.is_none()
        ),
        "`null` is a run nothing was issued, which is every run but a crashed one"
    );
    assert!(
        recorded(&line("{}")).is_err(),
        "a record that is not whole is off the published schema, so the recording is refused \
         rather than read as holding nothing"
    );
}

#[test]
fn a_reported_site_holds_what_its_decision_does_not_say_as_empty_and_refuses_another_shape() {
    let site = |decision: serde_json::Value| {
        xtask::crashes::site(&serde_json::json!({
            "display_id": "d".repeat(20),
            "decision": decision,
            "sealed": false,
        }))
    };
    let unreached = site(serde_json::json!({ "decision": "unreached" }))
        .expect("an unreached site says no target, which is its shape");
    assert_eq!(
        xtask::crashes::site(&serde_json::json!({
            "display_id": "d".repeat(20),
            "decision": { "decision": "unreached" },
        })),
        None,
        "a site that does not say whether it was sealed is not read as one that was not"
    );
    assert!(
        unreached.on.is_empty() && unreached.left.is_empty() && unreached.failed.is_empty(),
        "a decision that does not say `on`, `left` or `failed` is held as saying none, as a site \
         the steps decide is: {unreached:?}"
    );
    assert_eq!(
        site(serde_json::json!({ "decision": "restarted", "on": 7, "left": [] })),
        None,
        "an `on` that is there and is not a name is not read as no name"
    );
    assert_eq!(
        site(serde_json::json!({ "on": "pkg/lib/pkg t" })),
        None,
        "a decision that names no decision is not read as one"
    );
}
