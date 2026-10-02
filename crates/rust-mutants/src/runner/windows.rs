// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Engine diagnostics over the shared native ownership boundary.

use std::process::ExitStatus;

use super::{ProcessExit, SupervisionBoundary};
pub(super) use njutest_process::{configure_reader, stop_process, stream_ended};

pub(super) const SUPERVISOR_KIND: &str = "job-object";
pub(super) const SUPERVISION_BOUNDARY: SupervisionBoundary = SupervisionBoundary::ContainedTree;

pub(super) fn raised_by_itself(_signal: i32) -> bool {
    false
}

/// The child's status; a process the job terminated is reported by the caller as unavailable, so only a real exit reaches here.
pub(super) fn process_exit(status: ExitStatus) -> ProcessExit {
    match status.code() {
        Some(code) => ProcessExit::Code(code),
        None => ProcessExit::Unknown,
    }
}
