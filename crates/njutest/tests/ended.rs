// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a phase's process ended, and which of those endings is an answer about the suite.

use njutest::assure::ended::ProcessEnd;
use rust_mutants::runner::{ProcessExit, Termination};

#[test]
fn only_a_process_that_exited_with_a_code_of_its_own_answered_anything() {
    assert_eq!(
        ProcessEnd::of(&Termination::Exited(ProcessExit::Code(0))),
        ProcessEnd::Passed
    );
    assert_eq!(
        ProcessEnd::of(&Termination::Exited(ProcessExit::Code(101))),
        ProcessEnd::Failed
    );
    assert_eq!(
        ProcessEnd::of(&Termination::Cancelled { started: true }),
        ProcessEnd::Interrupted,
        "a stop the run asked for is the run being interrupted, whatever the process was doing"
    );
    assert_eq!(
        ProcessEnd::of(&Termination::TimedOut),
        ProcessEnd::TimedOut,
        "a phase that ran out of time answered nothing, which is not a toolchain that is absent"
    );
    for ended in [
        Termination::Exited(ProcessExit::Signal(9)),
        Termination::Exited(ProcessExit::Unknown),
        Termination::Stalled,
        Termination::StoppedByMonitor,
        Termination::Answered,
    ] {
        assert!(
            matches!(ProcessEnd::of(&ended), ProcessEnd::Unanswered { .. }),
            "{ended:?} is no answer about the suite and no answer about the toolchain"
        );
    }
}
