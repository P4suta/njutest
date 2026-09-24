// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crash's decision is re-derived from its ordered runs, so a report cannot claim more than they show.

use xtask::crashes::{Run, Site, agrees};

fn run(stage: &str, exit_code: i64, outcome: &str, left: &[&str]) -> Run {
    Run {
        crash: "dddd".to_owned(),
        target: "pkg/test/it".to_owned(),
        test: "t".to_owned(),
        stage: stage.to_owned(),
        exit_code,
        outcome: outcome.to_owned(),
        left: left.iter().map(|one| (*one).to_owned()).collect(),
        failed: Vec::new(),
    }
}

fn said(decision: &str, left: &[&str]) -> Site {
    Site {
        crash: "dddd".to_owned(),
        decision: decision.to_owned(),
        on: "pkg/test/it::t".to_owned(),
        left: left.iter().map(|one| (*one).to_owned()).collect(),
        failed: Vec::new(),
    }
}

fn holds(site: &Site, runs: &[Run]) -> bool {
    agrees(site, &runs.iter().collect::<Vec<_>>())
}

#[test]
fn a_decision_its_runs_do_not_show_is_refused_and_an_undecided_one_never_is() {
    let refused = [
        (
            "unreached whose crash run waited",
            Site {
                on: String::new(),
                ..said("unreached", &[])
            },
            vec![run("crash", 124, "waited", &[])],
        ),
        (
            "restarted whose next run failed",
            said("restarted", &["count"]),
            vec![
                run("crash", 93, "killed", &["count"]),
                run("next", 101, "killed", &[]),
            ],
        ),
        (
            "unshared whose stop left a file",
            said("unshared", &[]),
            vec![
                run("crash", 93, "killed", &["count"]),
                run("next", 0, "survived", &[]),
            ],
        ),
        (
            "corrupt whose fresh run failed",
            said("corrupt", &[]),
            vec![
                run("crash", 93, "killed", &["count"]),
                run("next", 101, "killed", &[]),
                run("fresh", 101, "killed", &[]),
                run("crash", 93, "killed", &["count"]),
                run("next", 101, "killed", &[]),
            ],
        ),
        (
            "restarted with other files than its stop left",
            said("restarted", &["other"]),
            vec![
                run("crash", 93, "killed", &["count"]),
                run("next", 0, "survived", &[]),
            ],
        ),
    ];
    for (case, site, runs) in &refused {
        assert!(!holds(site, runs), "{case} is refused");
    }
    assert!(
        holds(
            &said("restarted", &["count"]),
            &[
                run("crash", 93, "killed", &["count"]),
                run("next", 0, "survived", &[])
            ]
        ),
        "and what the runs do show holds"
    );
    assert!(
        holds(
            &said("undecided", &[]),
            &[run("crash", 93, "killed", &["count"])]
        ),
        "an undecided crash claims less than any run shows"
    );
}
