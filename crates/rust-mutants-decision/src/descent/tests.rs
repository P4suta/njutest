// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use core::num::NonZeroU32;
use std::collections::BTreeMap;
use std::vec;
use std::vec::Vec;

use super::{Descent, Link, Scope, could_have_started, descent};

const ENGINE: NonZeroU32 = NonZeroU32::new(100).expect("a process id");

fn table(rows: &[(u32, u32, u64)]) -> BTreeMap<u32, Link> {
    rows.iter()
        .map(|&(pid, parent, born)| (pid, Link { parent, born }))
        .collect()
}

fn walked(pid: u32, rows: &[(u32, u32, u64)], bound: u32) -> (Descent, Vec<u32>) {
    let table = table(rows);
    let mut asked = Vec::new();
    let found = descent(pid, ENGINE, bound, |each| {
        asked.push(each);
        Ok::<_, ()>(table.get(&each).copied())
    })
    .expect("a table that always answers");
    (found, asked)
}

const SESSION: &[(u32, u32, u64)] = &[
    (1, 0, 0),
    (2, 0, 0),
    (90, 1, 5),
    (100, 90, 10),
    (110, 100, 20),
    (120, 110, 30),
    (130, 1, 25),
    (140, 130, 40),
    (150, 2, 45),
];

#[test]
fn a_process_whose_parents_reach_the_engine_descends_from_it() {
    assert_eq!(
        walked(120, SESSION, 64),
        (Descent::Reaches(2), vec![120, 110])
    );
    assert_eq!(walked(110, SESSION, 64), (Descent::Reaches(1), vec![110]));
}

#[test]
fn a_process_whose_parents_end_without_the_engine_does_not_descend_from_it() {
    assert_eq!(
        walked(140, SESSION, 64),
        (Descent::Apart, vec![140, 130, 1])
    );
    assert_eq!(walked(150, SESSION, 64), (Descent::Apart, vec![150, 2]));
    assert_eq!(
        walked(90, SESSION, 64),
        (Descent::Apart, vec![90, 1]),
        "the engine's own parent is older than the engine and is not its descendant"
    );
}

#[test]
fn the_engine_is_reached_without_reading_anything() {
    assert_eq!(walked(100, SESSION, 64), (Descent::Reaches(0), vec![]));
}

#[test]
fn a_process_the_table_does_not_hold_has_ended() {
    assert_eq!(walked(999, SESSION, 64), (Descent::Ended, vec![999]));
}

#[test]
fn a_parent_the_table_does_not_hold_says_nothing_about_descent() {
    let vanished = [(120, 110, 30), (100, 90, 10)];
    assert_eq!(
        walked(120, &vanished, 64),
        (Descent::Broken, vec![120, 110])
    );
}

#[test]
fn a_parent_younger_than_its_child_names_a_reused_id_and_says_nothing() {
    let reused_under_the_engine = [(120, 110, 30), (110, 100, 31), (100, 90, 10)];
    assert_eq!(
        walked(120, &reused_under_the_engine, 64),
        (Descent::Broken, vec![120, 110]),
        "the process 110 named as the parent started after its child, so it is not the parent the \
         child had, and the engine it leads to is no evidence"
    );
    let reused_elsewhere = [(120, 110, 30), (110, 1, 31), (1, 0, 0)];
    assert_eq!(
        walked(120, &reused_elsewhere, 64),
        (Descent::Broken, vec![120, 110]),
        "a reused id that leads away from the engine would let a process it started pass as a \
         stranger"
    );
}

#[test]
fn a_parent_that_started_in_the_same_tick_as_its_child_is_its_parent() {
    let same_tick = [(120, 110, 30), (110, 100, 30)];
    assert_eq!(
        walked(120, &same_tick, 64),
        (Descent::Reaches(2), vec![120, 110])
    );
}

#[test]
fn parents_that_run_past_the_bound_say_nothing() {
    let cycle = [(120, 110, 30), (110, 120, 30)];
    assert_eq!(walked(120, &cycle, 8).0, Descent::Broken);
    assert_eq!(walked(120, SESSION, 1), (Descent::Broken, vec![120, 110]));
    assert_eq!(
        walked(120, SESSION, 2),
        (Descent::Reaches(2), vec![120, 110])
    );
    assert_eq!(
        walked(140, SESSION, 2),
        (Descent::Broken, vec![140, 130, 1])
    );
    assert_eq!(walked(140, SESSION, 3), (Descent::Apart, vec![140, 130, 1]));
    assert_eq!(walked(120, SESSION, 0), (Descent::Broken, vec![120]));
}

#[test]
fn an_engine_that_is_the_first_process_is_every_process_s_ancestor() {
    let first = NonZeroU32::new(1).expect("a process id");
    let rows = table(&[(1, 0, 0), (130, 1, 25), (140, 130, 40)]);
    let found = descent(140, first, 64, |each| Ok::<_, ()>(rows.get(&each).copied()));
    assert_eq!(found, Ok(Descent::Reaches(2)));
}

#[test]
fn a_table_that_cannot_be_read_is_the_failure() {
    let rows = table(SESSION);
    let found = descent(120, ENGINE, 64, |each| {
        if each == 110 {
            Err("the table refused")
        } else {
            Ok(rows.get(&each).copied())
        }
    });
    assert_eq!(found, Err("the table refused"));
}

const OWN: Scope = Scope {
    group: 100,
    session: 7,
};

#[test]
fn every_child_a_process_starts_could_be_one_it_started() {
    for (pid, scope, shape) in [
        (
            300,
            Scope {
                group: 300,
                session: 7,
            },
            "leads a process group of its own in the starter's session",
        ),
        (
            300,
            Scope {
                group: 300,
                session: 300,
            },
            "leads a session of its own",
        ),
        (300, OWN, "stays in the starter's group and session"),
    ] {
        assert!(could_have_started(pid, scope, OWN), "a child that {shape}");
    }
}

#[test]
fn a_child_in_a_group_or_session_no_started_child_is_in_was_handed_over() {
    for (scope, shape) in [
        (
            Scope {
                group: 250,
                session: 7,
            },
            "a group another process leads",
        ),
        (
            Scope {
                group: 250,
                session: 250,
            },
            "a group and a session another process leads",
        ),
        (
            Scope {
                group: 300,
                session: 250,
            },
            "a session another process leads",
        ),
        (
            Scope {
                group: 100,
                session: 250,
            },
            "the starter's group in another process's session",
        ),
    ] {
        assert!(
            !could_have_started(300, scope, OWN),
            "a child in {shape} cannot be one the starter started"
        );
    }
}
