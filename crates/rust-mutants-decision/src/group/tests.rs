// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec::Vec;

use super::{Delivered, Others, StopDecision, Stopped, agrees, decide_stop};

fn every_answer() -> Vec<(Delivered, Delivered, Others)> {
    let mut every = Vec::new();
    for group in Delivered::ALL {
        for leader in Delivered::ALL {
            for others in Others::ALL {
                every.push((group, leader, others));
            }
        }
    }
    every
}

fn every_decision() -> [StopDecision; 3] {
    [
        StopDecision::Reached(Stopped::Group),
        StopDecision::Reached(Stopped::LeaderOnly),
        StopDecision::Failed,
    ]
}

fn by_the_table(group: Delivered, leader: Delivered, others: Others) -> StopDecision {
    if group == Delivered::Failed {
        return StopDecision::Failed;
    }
    if group != Delivered::Refused {
        return StopDecision::Reached(Stopped::Group);
    }
    if leader == Delivered::Refused || leader == Delivered::Failed {
        return StopDecision::Failed;
    }
    if others == Others::Nobody {
        StopDecision::Reached(Stopped::Group)
    } else {
        StopDecision::Reached(Stopped::LeaderOnly)
    }
}

#[test]
fn every_answer_comes_to_the_decision_the_rule_gives() {
    let disagreeing: Vec<_> = every_answer()
        .into_iter()
        .filter(|(group, leader, others)| {
            decide_stop(*group, *leader, *others) != by_the_table(*group, *leader, *others)
        })
        .collect();
    assert!(
        disagreeing.is_empty(),
        "{} of {} answers are decided against the table, the first {:?}",
        disagreeing.len(),
        every_answer().len(),
        disagreeing.first()
    );
}

#[test]
fn the_check_accepts_exactly_the_decision_the_table_gives() {
    for (group, leader, others) in every_answer() {
        for decision in every_decision() {
            assert_eq!(
                agrees(group, leader, others, decision),
                decision == by_the_table(group, leader, others),
                "{decision:?} for group {group:?}, leader {leader:?}, others {others:?}"
            );
        }
    }
}

#[test]
fn a_refused_group_with_somebody_left_or_unseen_is_never_reached_whole() {
    for leader in [Delivered::Sent, Delivered::Gone] {
        for others in [Others::Somebody, Others::Unseen] {
            assert_eq!(
                decide_stop(Delivered::Refused, leader, others),
                StopDecision::Reached(Stopped::LeaderOnly),
                "a group the kernel refused is stopped only as far as its leader, unless a look \
                 at it finds nobody else: leader {leader:?}, others {others:?}"
            );
            assert!(
                !agrees(
                    Delivered::Refused,
                    leader,
                    others,
                    StopDecision::Reached(Stopped::Group)
                ),
                "the check refuses a whole group for a refusal with others {others:?}"
            );
        }
    }
}
