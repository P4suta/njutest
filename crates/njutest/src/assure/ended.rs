// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a process an assurance phase started ended, sorted by what that says about the suite it ran.

use rust_mutants::runner::{ProcessExit, Termination};

/// How a process a phase started ended, as far as that is an answer about the suite or the toolchain rather than about the run that started it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEnd {
    /// It exited 0.
    Passed,
    /// It exited with a code other than 0, which the suite or the toolchain chose.
    Failed,
    /// Nothing was launched, because the command, its launch or its first supervision failed, which is a toolchain that could not be run.
    Unlaunched {
        /// Why, in the runner's words.
        why: String,
    },
    /// The run was asked to stop, so nothing the process said is an answer.
    Interrupted,
    /// The phase's own time bound ended it before it answered.
    TimedOut,
    /// It ended some way that is no answer about the suite: a signal, a status nothing classified, supervision that failed, or a stop nobody here asked for.
    Unanswered {
        /// How, in words.
        how: String,
    },
}

impl ProcessEnd {
    /// How a process that ended as `termination` ended.
    #[must_use]
    pub fn of(termination: &Termination) -> Self {
        match termination {
            Termination::Exited(ProcessExit::Code(0)) => Self::Passed,
            Termination::Exited(ProcessExit::Code(_)) => Self::Failed,
            Termination::Exited(ProcessExit::Signal(signal)) => Self::Unanswered {
                how: format!("it was ended by signal {signal}"),
            },
            Termination::Exited(ProcessExit::Unknown) => Self::Unanswered {
                how: "it exited with a status nothing could classify".to_owned(),
            },
            Termination::NotStarted { error } => Self::Unlaunched {
                why: error.to_string(),
            },
            Termination::WaitFailed { error } => Self::Unanswered {
                how: error.to_string(),
            },
            Termination::MonitorFailed { failure } => Self::Unanswered {
                how: failure.to_string(),
            },
            Termination::TimedOut => Self::TimedOut,
            Termination::Stalled => Self::Unanswered {
                how: "it made no progress for its quiet window".to_owned(),
            },
            Termination::StoppedByMonitor => Self::Unanswered {
                how: "its execution monitor stopped it".to_owned(),
            },
            Termination::Answered => Self::Unanswered {
                how: "it was stopped at the first failure it named".to_owned(),
            },
            Termination::Cancelled { .. } => Self::Interrupted,
        }
    }
}
