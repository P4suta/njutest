// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes a run started that left every execution's process group and outlived it, found by the directory they work in.

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use njutest_process::{Asked, procfs};

/// Every process other than this one working under one of `dirs`, which only a process the run started does once its executions have ended.
///
/// # Errors
/// The process table could not be read at all; a process that ended or cannot be read while it is listed is not one.
pub fn working_under(dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let roots = roots(dirs)?;
    let mine = std::process::id();
    Ok(working_directories()?
        .into_iter()
        .filter(|(pid, cwd)| *pid != mine && roots.iter().any(|root| cwd.starts_with(root)))
        .map(|(pid, _cwd)| pid)
        .collect())
}

/// Settles every exact process generation observed under the owned roots before returning its identities.
///
/// # Errors
/// Identity, cancellation or complete exit could not be established for any observed producer.
pub fn end_working_under(dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let roots = roots(dirs)?;
    let deadline = std::time::Instant::now()
        .checked_add(crate::runner::REAPING_GRACE)
        .ok_or_else(|| {
            std::io::Error::other("the escaped-producer completion deadline exceeds the clock")
        })?;
    let mut ended = std::collections::BTreeSet::new();
    loop {
        let found = working_under(dirs)?;
        if found.is_empty() {
            return Ok(ended.into_iter().collect());
        }
        let mut failures = Vec::new();
        for pid in found {
            match settle(pid, &roots) {
                Ok(()) => {
                    ended.insert(pid);
                }
                Err(source) => failures.push(format!("process {pid}: {source}")),
            }
        }
        if !failures.is_empty() {
            return Err(std::io::Error::other(failures.join("; ")));
        }
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "escaped producers kept appearing after actual predecessor exit events",
            ));
        }
    }
}

fn settle(pid: u32, roots: &[PathBuf]) -> std::io::Result<()> {
    let Some(process) = njutest_process::ForeignProcess::retain(pid)? else {
        return Err(std::io::Error::other(format!(
            "observed escaped producer {pid} disappeared before its kernel exit was subscribed"
        )));
    };
    let matches = working_directories()?
        .into_iter()
        .any(|(actual, cwd)| actual == pid && roots.iter().any(|root| cwd.starts_with(root)));
    if !matches {
        return Err(std::io::Error::other(format!(
            "retained escaped producer {pid} no longer matches its owned source roots"
        )));
    }
    process.stop()?;
    Ok(())
}

fn roots(dirs: &[&Path]) -> std::io::Result<Vec<PathBuf>> {
    dirs.iter()
        .map(|dir| match std::fs::canonicalize(dir) {
            Ok(root) => Ok(root),
            Err(source)
                if source.kind() == std::io::ErrorKind::NotFound
                    && dir.is_absolute()
                    && dir
                        .components()
                        .all(|component| !matches!(component, std::path::Component::ParentDir)) =>
            {
                Ok((*dir).to_path_buf())
            }
            Err(source) => Err(std::io::Error::new(
                source.kind(),
                format!(
                    "cannot establish the owned producer root {}: {source}",
                    dir.display()
                ),
            )),
        })
        .collect()
}

/// Every process of this user beside the directory it works in, as `/proc` says.
#[cfg(target_os = "linux")]
fn working_directories() -> std::io::Result<Vec<(u32, PathBuf)>> {
    let own_start = match start_ticks(std::process::id())? {
        Asked::Answered(started) => started,
        Asked::Gone => {
            return Err(std::io::Error::other(
                "the process reading the process table is not in it",
            ));
        }
    };
    let mut found = Vec::new();
    for pid in procfs::processes()? {
        match procfs::owner(pid)? {
            Asked::Answered(owner) if owner == rustix::process::getuid().as_raw() => {}
            Asked::Answered(_) | Asked::Gone => continue,
        }
        match procfs::working_directory(pid) {
            Ok(Asked::Answered(cwd)) => {
                use std::os::unix::ffi::OsStrExt as _;
                let cwd = match cwd.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
                    Some(original) => PathBuf::from(std::ffi::OsStr::from_bytes(original)),
                    None => cwd,
                };
                found.push((pid, cwd));
            }
            Ok(Asked::Gone) => {}
            Err(source) if source.kind() == std::io::ErrorKind::PermissionDenied => {
                refused(source, start_ticks(pid)?, own_start)?;
            }
            Err(source) => return Err(source),
        }
    }
    Ok(found)
}

/// What a process whose working directory `/proc` refused comes to: nothing where it has gone or started before this process, which no run of it started, and the refusal otherwise.
#[cfg(target_os = "linux")]
fn refused(source: std::io::Error, started: Asked<u64>, own_start: u64) -> std::io::Result<()> {
    match started {
        Asked::Answered(started) if started >= own_start => Err(source),
        Asked::Answered(_) | Asked::Gone => Ok(()),
    }
}

/// When `/proc` says the process `pid` started, in clock ticks since boot: one started before this process cannot be a producer any run of it started.
#[cfg(target_os = "linux")]
fn start_ticks(pid: u32) -> std::io::Result<Asked<u64>> {
    match procfs::stat(pid)? {
        Asked::Answered(stat) => start_ticks_of(&stat).map(Asked::Answered),
        Asked::Gone => Ok(Asked::Gone),
    }
}

/// The start time field of one `/proc/<pid>/stat` line, read after the command name, which may itself hold spaces and parentheses.
#[cfg(target_os = "linux")]
fn start_ticks_of(stat: &str) -> std::io::Result<u64> {
    let fields = stat
        .rsplit_once(')')
        .map(|(_name, fields)| fields)
        .ok_or_else(|| std::io::Error::other("a process status line without its command name"))?;
    fields
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| std::io::Error::other("a process status line without its start time"))?
        .parse::<u64>()
        .map_err(std::io::Error::other)
}

/// Every process of this user beside the directory it works in, as `lsof` says.
#[cfg(all(unix, not(target_os = "linux")))]
fn working_directories() -> std::io::Result<Vec<(u32, PathBuf)>> {
    let user = rustix::process::getuid().as_raw().to_string();
    let mut spec = crate::runner::Spec::new(
        ["/usr/sbin/lsof", "-a", "-u", &user, "-d", "cwd", "-Fpn"],
        crate::runner::Bound::After(crate::runner::PROBE),
    );
    spec.structured_stdout = Some(LISTING_LIMIT);
    let result = crate::runner::run(&spec, &crate::runner::Cancel::new());
    if !result.succeeded() || result.stdout_truncated || result.stdout.is_empty() {
        return Err(std::io::Error::other(format!(
            "lsof did not establish the owned user's complete working-directory observation: {:?}; stderr: {:?}",
            result.termination,
            std::str::from_utf8(&result.output)
        )));
    }
    let text = std::str::from_utf8(&result.stdout).map_err(std::io::Error::other)?;
    let mut found = Vec::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = match pid.parse::<u32>() {
                Ok(pid) => Some(pid),
                Err(source) => return Err(std::io::Error::other(source)),
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

/// Ends `pid` at once, where it is still there, through the runner, the one place that signals a process by id.
///
/// # Errors
/// The process could not be signalled for a reason other than having ended.
pub fn stop(pid: u32) -> std::io::Result<()> {
    crate::runner::stop_process(pid)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use njutest_process::Asked;

    #[test]
    fn a_start_time_is_read_after_a_command_name_holding_spaces_and_parentheses() {
        let stat = "4242 (a (b) c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10 18446744073709551615";
        assert_eq!(
            super::start_ticks_of(stat).expect("a complete status line"),
            987_654
        );
    }

    #[test]
    fn this_process_did_not_start_before_itself_and_its_parent_did() {
        let own = super::start_ticks(std::process::id()).expect("this process");
        let parent =
            super::start_ticks(std::os::unix::process::parent_id()).expect("the parent process");
        match (parent, own) {
            (Asked::Answered(parent), Asked::Answered(own)) => assert!(
                parent <= own,
                "the parent started at {parent}, after this process at {own}"
            ),
            answered => panic!("this process and its parent both run: {answered:?}"),
        }
    }

    #[test]
    fn a_reaped_process_has_gone_rather_than_failed_to_say_when_it_started() {
        let mut ended = njutest_process::GroupChild::start(&mut std::process::Command::new("true"))
            .expect("true starts");
        let pid = ended.id().expect("the unreaped leader");
        ended.wait().expect("true is reaped");
        assert_eq!(
            super::start_ticks(pid).expect("a reaped process is no failure"),
            Asked::Gone
        );
    }

    #[test]
    fn a_refused_directory_is_refused_only_for_a_process_that_started_with_or_after_this_one() {
        let refusal = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        for (started, refused) in [
            (Asked::Gone, false),
            (Asked::Answered(9), false),
            (Asked::Answered(10), true),
            (Asked::Answered(11), true),
        ] {
            assert_eq!(
                super::refused(refusal(), started, 10).is_err(),
                refused,
                "a process that started at {started:?} beside this one at 10"
            );
        }
    }
}
