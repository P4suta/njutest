// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What xtask's work makes of a group stop, for every answer the kernel and a look at the group can give, held to the table the engine's runner is held to as well.

#![expect(
    clippy::panic,
    reason = "a test reports a malformed table by panicking"
)]

use std::collections::BTreeSet;

use xtask::work::{Delivered, Others, StopDecision, Stopped, decide_stop};

fn delivered(word: &str) -> Delivered {
    match word {
        "sent" => Delivered::Sent,
        "gone" => Delivered::Gone,
        "refused" => Delivered::Refused,
        "failed" => Delivered::Failed,
        other => panic!("the table names no delivery {other:?}"),
    }
}

fn others(word: &str) -> Others {
    match word {
        "nobody" => Others::Nobody,
        "somebody" => Others::Somebody,
        "unseen" => Others::Unseen,
        other => panic!("the table names no others {other:?}"),
    }
}

fn decision(word: &str) -> StopDecision {
    match word {
        "group" => StopDecision::Reached(Stopped::Group),
        "leader-only" => StopDecision::Reached(Stopped::LeaderOnly),
        "failed" => StopDecision::Failed,
        other => panic!("the table names no decision {other:?}"),
    }
}

#[test]
fn every_answer_a_group_stop_can_meet_comes_to_what_the_table_says() {
    let table = std::fs::read_to_string(
        njutest_devkit::paths::workspace_root()
            .join("crates/rust-mutants/tests/testdata/group-stop.tsv"),
    )
    .unwrap_or_else(|error| panic!("the shared table is readable: {error}"));
    let mut seen = BTreeSet::new();
    for line in table
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("group\t"))
    {
        let [group, leader, other, expected] = line.split('\t').collect::<Vec<&str>>()[..] else {
            panic!("a row has four cells: {line:?}")
        };
        let asked = (delivered(group), delivered(leader), others(other));
        assert!(
            seen.insert(format!("{asked:?}")),
            "{line:?} is listed twice"
        );
        assert_eq!(
            decide_stop(asked.0, asked.1, asked.2),
            decision(expected),
            "{line}"
        );
    }
    let every: BTreeSet<String> = Delivered::ALL
        .iter()
        .flat_map(|group| {
            Delivered::ALL.iter().flat_map(move |leader| {
                Others::ALL
                    .iter()
                    .map(move |other| format!("{:?}", (*group, *leader, *other)))
            })
        })
        .collect();
    assert_eq!(
        seen, every,
        "the table holds every answer the three closed sets can give, each once, so a new answer \
         is a row somebody writes rather than a case nobody decided"
    );
}

#[test]
fn a_refused_group_is_reached_whole_only_when_nobody_besides_its_leader_is_seen() {
    for other in Others::ALL {
        let reached = decide_stop(Delivered::Refused, Delivered::Sent, other);
        assert_eq!(
            reached == StopDecision::Reached(Stopped::Group),
            other == Others::Nobody,
            "a group the kernel refused is stopped only as far as its leader, unless a look at it \
             finds nobody else; one that could not be looked at is not taken for empty: {other:?}"
        );
    }
}
