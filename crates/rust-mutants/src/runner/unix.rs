// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Engine diagnostics over the shared native ownership boundary.

use std::io;
use std::process::ExitStatus;

use super::{ProcessExit, SupervisionBoundary};
pub(super) use njutest_process::{configure_reader, stop_process, stream_ended};

pub(super) const SUPERVISOR_KIND: &str = "process-group";
pub(super) const SUPERVISION_BOUNDARY: SupervisionBoundary =
    SupervisionBoundary::InheritedProcessGroup;

use rustix::process::Signal;
use std::os::unix::process::ExitStatusExt as _;

pub(super) fn stop_group(
    leader: njutest_process::Leader<'_>,
    how: super::GroupStop,
) -> io::Result<super::Stopped> {
    njutest_process::stop_group(
        leader,
        match how {
            super::GroupStop::Ask => njutest_process::GroupStop::Ask,
            super::GroupStop::Kill => njutest_process::GroupStop::Kill,
        },
    )
}

/// Whether `signal` is one a process raises by what it does (an abort, a bad access, a bad instruction, a trap) rather than one another process sends it.
pub(super) fn raised_by_itself(signal: i32) -> bool {
    [
        Signal::ABORT,
        Signal::SEGV,
        Signal::BUS,
        Signal::ILL,
        Signal::FPE,
        Signal::TRAP,
        Signal::SYS,
    ]
    .iter()
    .any(|raised| raised.as_raw() == signal)
}

/// The child's status, mapping a signal death to the shell's 128 + N convention: 137 for a SIGKILL is both distinguishable from "no status at all" and what every other tool on the machine prints.
pub(super) fn process_exit(status: ExitStatus) -> ProcessExit {
    match (status.code(), status.signal()) {
        (Some(code), _) => ProcessExit::Code(code),
        (None, Some(signal)) => ProcessExit::Signal(signal),
        (None, None) => ProcessExit::Unknown,
    }
}
