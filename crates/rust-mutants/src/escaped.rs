// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes a run started that left every execution's process group and outlived it, found by the directory they work in.

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use njutest_process::{Asked, procfs};

/// The proof that this process adopts every process it starts, which the census of a process whose working directory is hidden rests on.
#[derive(Debug, Clone, Copy)]
pub struct Adoption(());

/// Makes this process adopt every process it starts, so one the run started stays among its descendants until it ends.
///
/// # Errors
/// The kernel refused to make this process a reaper.
#[cfg(target_os = "linux")]
pub fn adopt() -> std::io::Result<Adoption> {
    njutest_process::adopt()?;
    Ok(Adoption(()))
}

/// The proof the census takes where it never asks whose a hidden process is.
///
/// # Errors
/// None here; the Linux adoption is refused by the kernel.
#[cfg(not(target_os = "linux"))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the Linux adoption, which the kernel can refuse"
)]
pub const fn adopt() -> std::io::Result<Adoption> {
    Ok(Adoption(()))
}

/// Reaps every ended process this one adopted that no process it started could be, and names each.
///
/// # Errors
/// The process table or an adopted process's end could not be read.
#[cfg(target_os = "linux")]
pub fn reap_adopted(_adoption: Adoption) -> std::io::Result<Vec<u32>> {
    njutest_process::reap_adopted()
}

/// Nothing to reap where a process ends up with whoever reaps above this one.
///
/// # Errors
/// None here; the Linux reaping reads the process table.
#[cfg(not(target_os = "linux"))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the Linux reaping, which reads the process table"
)]
pub const fn reap_adopted(_adoption: Adoption) -> std::io::Result<Vec<u32>> {
    Ok(Vec::new())
}

/// Every process other than this one working under one of `dirs`, which only a process the run started does once its executions have ended.
///
/// # Errors
/// The process table could not be read at all, or a process this one started works where `/proc` will not say; a process that ended or cannot be read while it is listed is not one.
pub fn working_under(adoption: Adoption, dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let roots = roots(dirs)?;
    let mine = std::process::id();
    Ok(working_directories(adoption)?
        .into_iter()
        .filter(|(pid, cwd)| *pid != mine && roots.iter().any(|root| cwd.starts_with(root)))
        .map(|(pid, _cwd)| pid)
        .collect())
}

/// Settles every exact process generation observed under the owned roots, then reaps those this process adopted, before returning their identities.
///
/// # Errors
/// Identity, cancellation, complete exit or reaping could not be established for any observed producer.
pub fn end_working_under(adoption: Adoption, dirs: &[&Path]) -> std::io::Result<Vec<u32>> {
    let roots = roots(dirs)?;
    let deadline = std::time::Instant::now()
        .checked_add(crate::runner::REAPING_GRACE)
        .ok_or_else(|| {
            std::io::Error::other("the escaped-producer completion deadline exceeds the clock")
        })?;
    let mut ended = std::collections::BTreeSet::new();
    loop {
        let found = working_under(adoption, dirs)?;
        if found.is_empty() {
            reap_adopted(adoption)?;
            return Ok(ended.into_iter().collect());
        }
        let mut failures = Vec::new();
        for pid in found {
            match settle(adoption, pid, &roots) {
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

fn settle(adoption: Adoption, pid: u32, roots: &[PathBuf]) -> std::io::Result<()> {
    let Some(process) = njutest_process::ForeignProcess::retain(pid)? else {
        return Err(std::io::Error::other(format!(
            "observed escaped producer {pid} disappeared before its kernel exit was subscribed"
        )));
    };
    let matches = working_directories(adoption)?
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

/// Every process of this user beside the directory it works in, as `/proc` says, where one whose directory `/proc` will not say is no process this one started.
#[cfg(target_os = "linux")]
fn working_directories(_adoption: Adoption) -> std::io::Result<Vec<(u32, PathBuf)>> {
    let listed = procfs::processes()?;
    let bound = u32::try_from(listed.len()).map_err(std::io::Error::other)?;
    let mut found = Vec::new();
    for pid in listed {
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
                hidden(pid, &source, bound)?;
            }
            Err(source) => return Err(source),
        }
    }
    Ok(found)
}

/// How many times the parents of a process whose working directory is hidden are read again after they changed under a reading, before the census refuses it.
#[cfg(target_os = "linux")]
const DESCENT_READINGS: usize = 8;

/// What a process whose working directory `/proc` refused with `source` comes to: nothing where it has ended or does not descend from this process, which adopts every process it starts so that one the run started stays its descendant, and the refusal, naming the process and its parents, where it descends or its parents keep changing under the reading.
#[cfg(target_os = "linux")]
fn hidden(pid: u32, source: &std::io::Error, bound: u32) -> std::io::Result<()> {
    use rust_mutants_decision::descent::{Descent, Link, descent};

    let engine = std::num::NonZeroU32::new(std::process::id())
        .ok_or_else(|| std::io::Error::other("this process has no id"))?;
    for _reading in 0..DESCENT_READINGS {
        let mut read = Vec::new();
        let found = descent(pid, engine, bound, |asked| {
            Ok::<_, std::io::Error>(match procfs::parsed(asked)? {
                Asked::Answered(stat) => {
                    let link = Link {
                        parent: stat.parent,
                        born: stat.born,
                    };
                    read.push(format!("{asked} ({})", stat.name));
                    Some(link)
                }
                Asked::Gone => None,
            })
        })?;
        match found {
            Descent::Reaches(_) => {
                let said = match read.split_first() {
                    Some((process, [])) => format!(
                        "process {process} works where /proc will not say, and descends from this \
                         process {engine} as its child"
                    ),
                    Some((process, through)) => format!(
                        "process {process} works where /proc will not say, and descends from this \
                         process {engine} through {}",
                        through.join(", ")
                    ),
                    None => format!("this process {engine} works where /proc will not say"),
                };
                return Err(std::io::Error::new(
                    source.kind(),
                    format!("{said}: {source}"),
                ));
            }
            Descent::Apart | Descent::Ended => return Ok(()),
            Descent::Broken => {}
        }
    }
    Err(std::io::Error::new(
        source.kind(),
        format!(
            "process {pid} works where /proc will not say, and its parents changed under each of \
             {DESCENT_READINGS} readings, so whether this process started it is unknown: {source}"
        ),
    ))
}

/// Every process of this user beside the directory it works in, as `lsof` says.
#[cfg(all(unix, not(target_os = "linux")))]
fn working_directories(_adoption: Adoption) -> std::io::Result<Vec<(u32, PathBuf)>> {
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
const fn working_directories(_adoption: Adoption) -> std::io::Result<Vec<(u32, PathBuf)>> {
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
