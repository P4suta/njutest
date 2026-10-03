// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes a run started that left every execution's process group and outlived it, found by the directory they work in.

use std::path::{Path, PathBuf};

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
    use std::os::unix::fs::MetadataExt as _;

    let mut found = Vec::new();
    for listed in std::fs::read_dir("/proc")? {
        let listed = listed?;
        let Some(Ok(pid)) = listed.file_name().to_str().map(str::parse::<u32>) else {
            continue;
        };
        let metadata = match listed.metadata() {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(source),
        };
        if metadata.uid() != rustix::process::getuid().as_raw() {
            continue;
        }
        match std::fs::read_link(listed.path().join("cwd")) {
            Ok(cwd) => {
                use std::os::unix::ffi::OsStrExt as _;
                let cwd = match cwd.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
                    Some(original) => PathBuf::from(std::ffi::OsStr::from_bytes(original)),
                    None => cwd,
                };
                found.push((pid, cwd));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(source),
        }
    }
    Ok(found)
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
