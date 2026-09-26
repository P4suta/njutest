// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes a run started that left every execution's process group and outlived it, found by the directory they work in.

use std::path::{Path, PathBuf};

/// Every process other than this one working under one of `dirs`, which only a process the run started does once its executions have ended.
///
/// # Errors
/// The process table could not be read at all; a process that ended or cannot be read while it is listed is not one.
pub fn working_under(dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let dirs: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|dir| match std::fs::canonicalize(dir) {
            Ok(dir) => Some(dir),
            Err(_gone) => None,
        })
        .collect();
    if dirs.is_empty() {
        return Ok(Vec::new());
    }
    let mine = std::process::id();
    Ok(working_directories()?
        .into_iter()
        .filter(|(pid, cwd)| *pid != mine && dirs.iter().any(|dir| cwd.starts_with(dir)))
        .map(|(pid, _)| pid)
        .collect())
}

/// Ends every process other than this one working under one of `dirs`, and says which it ended.
///
/// # Errors
/// The process table could not be read, or a process could not be signalled for a reason other than having ended.
pub fn end_working_under(dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let found = working_under(dirs)?;
    for pid in &found {
        stop(*pid)?;
    }
    Ok(found)
}

/// Every process of this user beside the directory it works in, as `/proc` says.
#[cfg(target_os = "linux")]
fn working_directories() -> std::io::Result<Vec<(u32, PathBuf)>> {
    let mut found = Vec::new();
    for listed in std::fs::read_dir("/proc")? {
        let Ok(listed) = listed else {
            continue;
        };
        let Some(Ok(pid)) = listed.file_name().to_str().map(str::parse::<u32>) else {
            continue;
        };
        match std::fs::read_link(listed.path().join("cwd")) {
            Ok(cwd) => found.push((pid, cwd)),
            Err(_ended_or_not_ours) => {}
        }
    }
    Ok(found)
}

/// Every process of this user beside the directory it works in, as `lsof` says.
#[cfg(all(unix, not(target_os = "linux")))]
fn working_directories() -> std::io::Result<Vec<(u32, PathBuf)>> {
    let mut spec = crate::runner::Spec::new(
        ["/usr/sbin/lsof", "-a", "-d", "cwd", "-Fpn"],
        crate::runner::Bound::After(crate::runner::PROBE),
    );
    spec.structured_stdout = Some(LISTING_LIMIT);
    let result = crate::runner::run(&spec, &crate::runner::Cancel::new());
    if result.stdout_truncated || result.stdout.is_empty() {
        return Err(std::io::Error::other(
            "lsof did not list every process's working directory",
        ));
    }
    let text = std::str::from_utf8(&result.stdout).map_err(std::io::Error::other)?;
    let mut found = Vec::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = match pid.parse::<u32>() {
                Ok(pid) => Some(pid),
                Err(_not_a_pid) => None,
            };
        } else if let (Some(pid), Some(cwd)) = (current, line.strip_prefix('n')) {
            found.push((pid, PathBuf::from(cwd)));
        }
    }
    Ok(found)
}

/// No process a run starts outlives it on Windows, where the execution's Job Object ends every one.
#[cfg(windows)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the unix readers, which can fail"
)]
const fn working_directories() -> std::io::Result<Vec<(u32, PathBuf)>> {
    Ok(Vec::new())
}

/// How much of `lsof`'s listing a run reads, which every process's working directory fits in.
#[cfg(all(unix, not(target_os = "linux")))]
const LISTING_LIMIT: usize = 16 * 1024 * 1024;

/// Ends `pid` at once, where it is still there.
///
/// # Errors
/// The process could not be signalled for a reason other than having ended.
#[cfg(unix)]
pub fn stop(pid: u32) -> std::io::Result<()> {
    let raw = match i32::try_from(pid) {
        Ok(raw) => raw,
        Err(_out_of_range) => return Ok(()),
    };
    let Some(pid) = rustix::process::Pid::from_raw(raw) else {
        return Ok(());
    };
    match rustix::process::kill_process(pid, rustix::process::Signal::KILL) {
        Ok(()) => Ok(()),
        Err(errno) if errno == rustix::io::Errno::SRCH => Ok(()),
        Err(errno) => Err(errno.into()),
    }
}

/// Nothing to end on Windows, where the Job Object ended it.
///
/// # Errors
/// None.
#[cfg(windows)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the unix stop, which can fail"
)]
pub const fn stop(_pid: u32) -> std::io::Result<()> {
    Ok(())
}
