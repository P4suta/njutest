// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run is called, and what two runs are called.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use jiff::Timestamp;
use njutest_cli::run_id::mint;

fn at(second: i64, milli: i64) -> Timestamp {
    Timestamp::from_millisecond(second.saturating_mul(1_000).saturating_add(milli))
        .expect("in range")
}

#[test]
fn two_runs_that_start_in_one_second_are_two_runs() {
    assert_ne!(
        mint(at(1_800_000_000, 0), 7),
        mint(at(1_800_000_000, 40), 7),
        "a name that counts only seconds gives two runs of one process one name, and the \
         second is stored over the first: the report a reader goes back to is then a \
         report of a run nobody asked about"
    );
}

#[test]
fn two_runs_that_start_at_one_instant_in_two_processes_are_two_runs() {
    assert_ne!(
        mint(at(1_800_000_000, 0), 7),
        mint(at(1_800_000_000, 8), 7 + 1),
        "two runs of one tree started together are two runs, and the machine tells them \
         apart by the process each is"
    );
}

#[test]
fn the_run_that_started_later_sorts_later() {
    let (first, second) = (
        mint(at(1_800_000_000, 0), 7),
        mint(at(1_800_000_000, 40), 7),
    );
    assert!(
        first < second,
        "what is kept and what is swept is decided by name order, so a name that did not \
         sort by time would sweep the run somebody wanted: {first} against {second}"
    );
}
