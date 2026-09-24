// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Build output and temporary directories nobody has touched for a while, taken back so a machine that develops all day does not fill up.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use thiserror::Error;

/// How long build output or a temporary directory sits unwritten before it is taken.
pub const IDLE: Duration = Duration::from_hours(6);

/// How long it sits unwritten before it is taken when the disk is nearly full.
pub const PRESSED_IDLE: Duration = Duration::from_mins(30);

/// The share of the disk, in percent, below which free space counts as nearly full.
pub const PRESSURE_PERCENT: u64 = 15;

/// How long the sweep at the end of a push may spend removing.
pub const AFTER_A_PUSH: Duration = Duration::from_secs(30);

/// The name of the directory taken things wait in, one per volume, until they are removed.
pub const TRASH: &str = ".njutest-trash";

/// The prefixes of the temporary directories only this repository's tests, devkit and gates make; the product makes others for somebody's own runs, and those are not this repository's to take.
pub const OURS: [&str; 7] = [
    "njutest-commands-",
    "njutest-devkit-",
    "njutest-fake-cargo-",
    "njutest-fixture-",
    "njutest-pre-push-",
    "njutest-repo-",
    "rust-mutants-stored-",
];

/// The prefix of the per-worktree gate trees the gate made before it had one tree per repository.
const GATE_TREE: &str = "njutest-pre-push-";

/// What one sweep is asked to look at.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// A worktree of the repository whose worktrees are looked at.
    pub repository: &'a Path,
    /// The temporary directory the tests and gates make their directories in.
    pub temp: &'a Path,
    /// The process environment, for the `git` it asks.
    pub environment: &'a [(OsString, OsString)],
    /// How long removing may take before the rest is left for the next sweep.
    pub budget: Duration,
    /// The time the sweep judges idleness against.
    pub now: SystemTime,
}

/// What a sweep took and what it left.
#[derive(Debug, Default)]
pub struct Swept {
    /// Every directory it took.
    pub taken: Vec<PathBuf>,
    /// Every idle directory it left because a process works in it or holds something under it.
    pub in_use: Vec<PathBuf>,
    /// Every directory it meant to take and could not, with what the system said.
    pub failed: Vec<(PathBuf, String)>,
    /// Whether nothing could say which directories are in use, so nothing idle was taken.
    pub blind: bool,
    /// How many files and directories it removed.
    pub removed: u64,
    /// Whether taken things are still waiting in a trash directory because the budget ran out.
    pub unfinished: bool,
    /// How long a thing had to sit unwritten to be taken this time.
    pub idle: Duration,
}

impl std::fmt::Display for Swept {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "sweep: took {} director{} unwritten for {} minutes or more and removed {} entries",
            self.taken.len(),
            if self.taken.len() == 1 { "y" } else { "ies" },
            self.idle.as_secs() / 60,
            self.removed
        )?;
        if self.unfinished {
            write!(
                formatter,
                "; the budget ran out and the rest waits in {TRASH} for the next sweep"
            )?;
        }
        if self.blind {
            write!(
                formatter,
                "; nothing could say which directories a process is using, so nothing idle was taken"
            )?;
        }
        for kept in &self.in_use {
            write!(formatter, "\n  kept, in use: {}", kept.display())?;
        }
        for (path, said) in &self.failed {
            write!(formatter, "\n  could not take {}: {said}", path.display())?;
        }
        Ok(())
    }
}

impl Swept {
    /// Whether it left something it meant to take, which the command's status says.
    #[must_use]
    pub const fn fell_short(&self) -> bool {
        !self.failed.is_empty()
    }
}

/// Why a sweep stopped.
#[derive(Debug, Error)]
pub enum SweepError {
    /// `git` could not list the worktrees.
    #[error("git worktree {action} failed in {}: {detail}", repository.display())]
    Git {
        /// What was asked of git.
        action: &'static str,
        /// Where.
        repository: PathBuf,
        /// What it said.
        detail: String,
    },
    /// A directory could not be read or moved.
    #[error("{}: {source}", path.display())]
    Io {
        /// The directory.
        path: PathBuf,
        /// The operating system's refusal.
        source: std::io::Error,
    },
}

/// Takes the build output and temporary directories nobody has written for a while, and removes what it took within the budget.
///
/// # Errors
/// Returns a [`SweepError`] when git cannot list the worktrees or a directory cannot be moved aside.
pub fn sweep(request: &Request<'_>) -> Result<Swept, SweepError> {
    let started = Instant::now();
    let idle = if pressed(request.temp) {
        PRESSED_IDLE
    } else {
        IDLE
    };
    let mut swept = Swept {
        idle,
        ..Swept::default()
    };
    let mut trashes: Vec<PathBuf> = vec![request.temp.join(TRASH)];
    let mut forgotten = false;
    let mut idle_ones: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
    for worktree in worktrees(request)? {
        let (candidate, holder, trash) = match gate_directory(&worktree, request.temp) {
            Some(gate) => (gate.clone(), gate, request.temp.join(TRASH)),
            None => (
                worktree.join("target"),
                worktree.clone(),
                worktree.join(TRASH),
            ),
        };
        if unwritten_for(&candidate, request.now).is_some_and(|quiet| quiet >= idle) {
            idle_ones.push((candidate, holder, trash));
        }
    }
    for leftover in ours(request.temp)? {
        let already = idle_ones
            .iter()
            .any(|(candidate, _holder, _trash)| *candidate == leftover);
        if !already && unwritten_for(&leftover, request.now).is_some_and(|quiet| quiet >= idle) {
            idle_ones.push((leftover.clone(), leftover, request.temp.join(TRASH)));
        }
    }
    if idle_ones.is_empty() {
        return Ok(swept);
    }
    let Some(held) = Held::now() else {
        swept.blind = true;
        return Ok(swept);
    };
    for (candidate, holder, trash) in idle_ones {
        let Some(unheld) = held.release(&candidate, &holder) else {
            swept.in_use.push(candidate);
            continue;
        };
        match take(unheld, &trash, request.now) {
            Ok(()) => {
                forgotten |= candidate.starts_with(request.temp);
                swept.taken.push(candidate);
                if !trashes.contains(&trash) {
                    trashes.push(trash);
                }
            }
            Err(failure) => swept.failed.push((candidate, failure.to_string())),
        }
    }
    if forgotten {
        git(request, "prune", &["worktree", "prune"])?;
    }
    let deadline = started.checked_add(request.budget);
    for trash in &trashes {
        let (removed, finished) = empty(trash, deadline);
        swept.removed = swept.removed.saturating_add(removed);
        swept.unfinished |= !finished;
    }
    Ok(swept)
}

/// `path` with every link resolved, which is how the operating system names what a process holds, or `path` as it is where it cannot be resolved.
fn canonical(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(_unresolved) => path.to_path_buf(),
    }
}

/// Every path some process on this machine has as its working directory, runs, or holds open, at one moment.
struct Held(Vec<PathBuf>);

/// A directory no process held when [`Held`] was read, which is the only thing [`take`] accepts.
struct Unheld(PathBuf);

impl Held {
    /// What is held now, or nothing where nothing could say.
    fn now() -> Option<Self> {
        in_use().map(Self)
    }

    /// `candidate`, where no process works in or holds anything under `holder`, the directory whose use keeps it.
    fn release(&self, candidate: &Path, holder: &Path) -> Option<Unheld> {
        let holder = canonical(holder);
        (!self.0.iter().any(|open| open.starts_with(&holder)))
            .then(|| Unheld(candidate.to_path_buf()))
    }
}

/// Every path some process on this machine has as its working directory, runs, or holds open, or nothing where nothing could say.
#[cfg(unix)]
fn in_use() -> Option<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStrExt;
    let listed = match Command::new("lsof")
        .args(["-w", "-n", "-P", "-F", "n"])
        .output()
    {
        Ok(listed) => listed,
        Err(_no_lsof) => return None,
    };
    if listed.stdout.is_empty() {
        return None;
    }
    Some(
        listed
            .stdout
            .split(|&byte| byte == b'\n')
            .filter_map(|line| line.strip_prefix(b"n/"))
            .map(|rest| {
                let mut named = b"/".to_vec();
                named.extend_from_slice(rest);
                PathBuf::from(std::ffi::OsStr::from_bytes(&named))
            })
            .collect(),
    )
}

/// Every path in use, which this platform cannot say, so nothing idle is taken.
#[cfg(not(unix))]
const fn in_use() -> Option<Vec<PathBuf>> {
    None
}

/// Whether the volume `path` is on has less than [`PRESSURE_PERCENT`] of its space free.
#[cfg(unix)]
fn pressed(path: &Path) -> bool {
    match rustix::fs::statvfs(path) {
        Ok(volume) => {
            let total = u128::from(volume.f_blocks);
            let free = u128::from(volume.f_bavail);
            total > 0
                && free.saturating_mul(100) < total.saturating_mul(u128::from(PRESSURE_PERCENT))
        }
        Err(_unreadable) => false,
    }
}

/// Whether the volume `path` is on is nearly full, which this platform cannot say.
#[cfg(not(unix))]
const fn pressed(_path: &Path) -> bool {
    false
}

/// Every worktree of the repository, as git lists them.
fn worktrees(request: &Request<'_>) -> Result<Vec<PathBuf>, SweepError> {
    let listed = git(request, "list", &["worktree", "list", "--porcelain"])?;
    Ok(listed
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect())
}

/// Runs git in the repository with the environment's own git variables removed, and hands back what it printed.
fn git(
    request: &Request<'_>,
    action: &'static str,
    arguments: &[&str],
) -> Result<String, SweepError> {
    let mut command = Command::new("git");
    for (name, _value) in request.environment {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    let failed = |detail: String| SweepError::Git {
        action,
        repository: request.repository.to_path_buf(),
        detail,
    };
    let output = command
        .args(arguments)
        .current_dir(request.repository)
        .output()
        .map_err(|source| failed(source.to_string()))?;
    if !output.status.success() {
        return Err(failed(match String::from_utf8(output.stderr) {
            Ok(said) => said,
            Err(_not_text) => "git said something that is not UTF-8".to_owned(),
        }));
    }
    String::from_utf8(output.stdout).map_err(|source| failed(source.to_string()))
}

/// The directory a per-worktree gate tree lives in, when `worktree` is one, which goes whole with its build output.
fn gate_directory(worktree: &Path, temp: &Path) -> Option<PathBuf> {
    let directory = worktree.parent()?;
    let named = directory.file_name()?.to_str()?;
    let (Ok(canonical_temp), Ok(canonical)) = (
        std::fs::canonicalize(temp),
        std::fs::canonicalize(directory),
    ) else {
        return None;
    };
    (named.starts_with(GATE_TREE) && canonical.parent() == Some(canonical_temp.as_path()))
        .then(|| temp.join(named))
}

/// The directories in `temp` this repository's tests and gates make.
fn ours(temp: &Path) -> Result<Vec<PathBuf>, SweepError> {
    let entries = match std::fs::read_dir(temp) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(SweepError::Io {
                path: temp.to_path_buf(),
                source,
            });
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SweepError::Io {
            path: temp.to_path_buf(),
            source,
        })?;
        let named = entry.file_name();
        let Some(named) = named.to_str() else {
            continue;
        };
        if OURS.iter().any(|prefix| named.starts_with(prefix)) {
            found.push(entry.path());
        }
    }
    Ok(found)
}

/// How long since anything at `path`, its entries, or theirs was written; nothing where `path` is not a directory.
fn unwritten_for(path: &Path, now: SystemTime) -> Option<Duration> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) | Err(_) => return None,
    }
    let newest = walkdir::WalkDir::new(path)
        .max_depth(2)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(_vanished) => None,
        })
        .filter_map(|entry| match entry.metadata() {
            Ok(metadata) => Some(metadata),
            Err(_vanished) => None,
        })
        .filter_map(|metadata| match metadata.modified() {
            Ok(modified) => Some(modified),
            Err(_unsupported) => None,
        })
        .max()?;
    match now.duration_since(newest) {
        Ok(quiet) => Some(quiet),
        Err(_in_the_future) => Some(Duration::ZERO),
    }
}

/// Moves `path` into `trash` on the same volume, which takes it out of use at once whatever its size.
fn take(Unheld(path): Unheld, trash: &Path, now: SystemTime) -> Result<(), SweepError> {
    let path = path.as_path();
    std::fs::create_dir_all(trash).map_err(|source| SweepError::Io {
        path: trash.to_path_buf(),
        source,
    })?;
    let stamp = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => since.as_nanos(),
        Err(_before_the_epoch) => 0,
    };
    let relative = match path.strip_prefix("/") {
        Ok(relative) => relative,
        Err(_already_relative) => path,
    };
    let named = relative.display().to_string().replace('/', "_");
    let destination = trash.join(format!("{named}-{stamp}"));
    std::fs::rename(path, &destination).map_err(|source| SweepError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Removes what waits in `trash` until `deadline`, and says how many entries went and whether it finished.
fn empty(trash: &Path, deadline: Option<Instant>) -> (u64, bool) {
    let mut removed: u64 = 0;
    let Ok(waiting) = std::fs::read_dir(trash) else {
        return (0, true);
    };
    for taken in waiting.filter_map(|entry| match entry {
        Ok(entry) => Some(entry.path()),
        Err(_vanished) => None,
    }) {
        for entry in walkdir::WalkDir::new(&taken).contents_first(true) {
            if deadline.is_some_and(|at| Instant::now() >= at) {
                return (removed, false);
            }
            let Ok(entry) = entry else {
                continue;
            };
            let gone = if entry.file_type().is_dir() {
                std::fs::remove_dir(entry.path())
            } else {
                std::fs::remove_file(entry.path())
            };
            if gone.is_ok() {
                removed = removed.saturating_add(1);
            }
        }
    }
    (removed, true)
}
