// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crash's decision is re-derived from its ordered steps exactly, so a report cannot claim more, or less, than they show.

use xtask::crashes::{Asked, Crashed, Run, Site, Step, Unmade, decided, disagreements};

fn run(test: &str, stage: &str, ended: &str, files: &[&str]) -> Step {
    let (exit_code, outcome) = match ended {
        "stopped" => (93, "killed"),
        "passed" => (0, "survived"),
        _ => (101, "killed"),
    };
    let files: Vec<String> = files.iter().map(|one| (*one).to_owned()).collect();
    let (left, failed) = if stage == "crash" {
        (files, Vec::new())
    } else {
        (Vec::new(), files)
    };
    Step::Ran(Run {
        target: "pkg/test/it".to_owned(),
        test: test.to_owned(),
        stage: stage.to_owned(),
        exit_code,
        outcome: outcome.to_owned(),
        left,
        failed,
    })
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
