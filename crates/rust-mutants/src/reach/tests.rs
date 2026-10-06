// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::measured_whole;
use crate::execute::{MutantResult, Protocol, Stopped};
use crate::runner::ProcessExit;

fn ran(output: &str, protocol: Protocol, stopped: Stopped, exit_code: i32) -> MutantResult {
    MutantResult {
        exit_code,
        output: output.as_bytes().to_vec(),
        protocol,
        stopped,
        ..MutantResult::apparatus_error("target", String::new())
    }
}

const fn exited(code: i32) -> Stopped {
    Stopped::Exited {
        exit: ProcessExit::Code(code),
    }
}

#[test]
fn a_coverage_run_that_ended_before_its_harness_closed_measures_no_reach() {
    let early = ran(
        "\nrunning 2 tests\ntest a ... ok\n",
        Protocol::Libtest,
        exited(0),
        0,
    );
    assert!(!measured_whole(&early));
}

#[test]
fn a_coverage_run_its_harness_accounted_for_measures_reach_whether_or_not_it_failed() {
    let passed = ran(
        "\nrunning 1 test\ntest a ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        Protocol::Libtest,
        exited(0),
        0,
    );
    let failed = ran(
        "\nrunning 1 test\ntest a ... FAILED\n\nfailures:\n    a\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        Protocol::Libtest,
        exited(101),
        101,
    );
    assert!(measured_whole(&passed));
    assert!(measured_whole(&failed));
}

#[test]
fn a_coverage_run_the_clock_ended_measures_no_reach() {
    let timed_out = ran("", Protocol::Libtest, Stopped::TimedOut { raised: None }, 0);
    assert!(!measured_whole(&timed_out));
}

#[test]
fn a_coverage_run_of_a_harness_that_answers_by_exit_status_measures_reach_when_it_exits() {
    let custom = ran("", Protocol::Custom, exited(0), 0);
    assert!(measured_whole(&custom));
}
