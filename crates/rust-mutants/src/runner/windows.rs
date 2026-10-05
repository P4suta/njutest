// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Engine diagnostics over the shared native ownership boundary.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Component, Path, Prefix};
use std::process::ExitStatus;

use super::{ProcessExit, SupervisionBoundary};
pub(super) use njutest_process::{configure_reader, stop_process, stream_ended};

pub(super) const SUPERVISOR_KIND: &str = "job-object";
pub(super) const SUPERVISION_BOUNDARY: SupervisionBoundary = SupervisionBoundary::ContainedTree;

/// The child's status; a process the job terminated is reported by the caller as unavailable, so only a real exit reaches here.
pub(super) fn process_exit(status: ExitStatus) -> ProcessExit {
    match status.code() {
        Some(code) => ProcessExit::Code(code),
        None => ProcessExit::Unknown,
    }
}

/// The length from which Windows process creation refuses a program path it was given as it is.
const START_PATH_LIMIT: usize = 260;

/// The spelling of `program` Windows starts: an absolute path at or past [`START_PATH_LIMIT`] in the verbatim form that has no such limit, and any other as it is.
pub(super) fn startable(program: OsString) -> OsString {
    let path = Path::new(&program);
    if !path.is_absolute() || program.encode_wide().count() < START_PATH_LIMIT {
        return program;
    }
    let Ok(full) = std::path::absolute(path) else {
        return program;
    };
    let wide: Vec<u16> = full.as_os_str().encode_wide().collect();
    let Some(Component::Prefix(prefix)) = full.components().next() else {
        return program;
    };
    let (marker, rest): (&str, &[u16]) = match prefix.kind() {
        Prefix::Disk(_) => (r"\\?\", &wide),
        Prefix::UNC(_, _) => match wide.split_first() {
            Some((_, after_one_separator)) => (r"\\?\UNC", after_one_separator),
            None => return program,
        },
        Prefix::Verbatim(_)
        | Prefix::VerbatimUNC(_, _)
        | Prefix::VerbatimDisk(_)
        | Prefix::DeviceNS(_) => return full.into_os_string(),
    };
    let spelled: Vec<u16> = marker.encode_utf16().chain(rest.iter().copied()).collect();
    OsString::from_wide(&spelled)
}
