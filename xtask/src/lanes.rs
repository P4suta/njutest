// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Machine-wide lanes that admit one whole-workspace run at a time, and say who holds each.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::work::Stops;

/// The variable that names the lanes a process already holds, so a run inside one never waits for itself.
pub const HELD: &str = "NJUTEST_SLOT_HELD";

/// How often a waiting run looks at the lock again.
const POLL: Duration = Duration::from_millis(200);

/// How often a waiting run repeats whom it is waiting for.
const REPORT: Duration = Duration::from_secs(30);

/// A lane a run can queue for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// A run that compiles or tests the whole workspace, one per machine.
    Heavy,
    /// The gate's tree and build, one per repository, taken whatever [`HELD`] says.
    Tree,
}

impl Lane {
    /// The lane's name on the command line, in its files, and in [`HELD`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Heavy => "heavy",
            Self::Tree => "tree",
        }
    }

    /// The machine-wide lane `name` spells, if it spells one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "heavy" => Some(Self::Heavy),
            _ => None,
        }
    }
}

/// What a lane's record says about the run holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The directory the run was started in.
    pub worktree: PathBuf,
    /// The branch and commit the run is about, as a person names them.
    pub revision: String,
    /// What the run runs.
    pub command: String,
}

/// One run asking for one lane.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The lane.
    pub lane: Lane,
    /// Who is asking, for the record and for whoever waits behind it.
    pub holder: &'a Holder,
    /// The signals that end the wait.
    pub stops: &'a Stops,
}

/// Why a lane could not be held.
#[derive(Debug, Error)]
pub enum LaneError {
    /// A variable the lanes read was not UTF-8.
    #[error("{name} is not UTF-8 text")]
    NotText {
        /// The variable.
        name: &'static str,
    },
    /// Nothing in the environment says where this machine keeps its lanes.
    #[error("no directory for the lanes: set NJUTEST_SLOT_DIR, XDG_STATE_HOME or HOME")]
    Nowhere,
    /// The lane's directory, lock, or record could not be written.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory that could not be written.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// The lock could not be taken, for a reason other than another run holding it.
    #[error("{path}: the lock could not be taken: {source}")]
    Lock {
        /// The lock file.
        path: String,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// Whom the run is waiting for could not be said.
    #[error("the progress of a waiting run could not be written: {source}")]
    Progress {
        /// The output failure.
        source: std::io::Error,
    },
    /// This process was asked to stop while it waited.
    #[error("stopped by signal {signal} while waiting for the {lane} lane")]
    Interrupted {
        /// The lane.
        lane: &'static str,
        /// The signal.
        signal: i32,
    },
}

impl crate::error::Coded for LaneError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::NotText { .. }
            | Self::Nowhere
            | Self::Io { .. }
            | Self::Lock { .. }
            | Self::Progress { .. } => crate::error::XtCode::LaneUnavailable,
            Self::Interrupted { .. } => crate::error::XtCode::LaneInterrupted,
        }
    }
}

impl LaneError {
    /// The signal that ended the wait, when one did.
    #[must_use]
    pub const fn signal(&self) -> Option<i32> {
        match self {
            Self::Interrupted { signal, .. } => Some(*signal),
            Self::NotText { .. }
            | Self::Nowhere
            | Self::Io { .. }
            | Self::Lock { .. }
            | Self::Progress { .. } => None,
        }
    }
}

/// Where lanes are kept, and which of them the running process already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lanes {
    directory: PathBuf,
    held: Vec<String>,
}

impl Lanes {
    /// The machine's lanes as the environment names them: `NJUTEST_SLOT_DIR`, else under the state directory, with [`HELD`] saying which are already held.
    ///
    /// # Errors
    /// Returns [`LaneError::Nowhere`] when the environment names no directory for them.
    pub fn from_environment(environment: &[(OsString, OsString)]) -> Result<Self, LaneError> {
        let directory = match variable(environment, "NJUTEST_SLOT_DIR") {
            Some(named) => PathBuf::from(named),
            None => state_directory(environment)
                .ok_or(LaneError::Nowhere)?
                .join("njutest")
                .join("slots"),
        };
        let held = match variable(environment, HELD) {
            Some(value) => value
                .to_str()
                .ok_or(LaneError::NotText { name: HELD })?
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        Ok(Self { directory, held })
    }

    /// Lanes kept in `directory` that nothing already holds, such as one repository's gate tree.
    #[must_use]
    pub const fn at(directory: PathBuf) -> Self {
        Self {
            directory,
            held: Vec::new(),
        }
    }

    /// The value of [`HELD`] for a child of a process that holds `lane`.
    #[must_use]
    pub fn held_with(&self, lane: Lane) -> String {
        let mut held = self.held.clone();
        if !held.iter().any(|name| name == lane.name()) {
            held.push(lane.name().to_owned());
        }
        held.join(",")
    }

    /// Waits until the lane is free and the work its last holder left behind has ended, telling `progress` whom it waits for, and holds the lane until the answer is dropped.
    ///
    /// # Errors
    /// Returns a [`LaneError`] when the lane's files cannot be written, its lock cannot be taken, or a signal ends the wait.
    pub fn hold(&self, request: &Request<'_>, progress: &mut dyn Write) -> Result<Held, LaneError> {
        let lane = request.lane;
        if self.held.iter().any(|name| name == lane.name()) {
            return Ok(Held {
                lock: None,
                record: None,
            });
        }
        std::fs::create_dir_all(&self.directory).map_err(|source| io(&self.directory, source))?;
        let lock_path = self.directory.join(format!("{}.lock", lane.name()));
        let record = self.directory.join(format!("{}.holder", lane.name()));
        let place = Place {
            directory: &self.directory,
            lock: &lock_path,
            record: &record,
        };
        let lock = place.take(request, progress)?;
        place.outlast(request, progress)?;
        replace(&record, &record_of(request.holder)).map_err(|source| io(&record, source))?;
        Ok(Held {
            lock: Some(lock),
            record: Some(record),
        })
    }
}

/// The files one lane lives in.
#[derive(Debug, Clone, Copy)]
struct Place<'a> {
    directory: &'a Path,
    lock: &'a Path,
    record: &'a Path,
}

impl Place<'_> {
    fn open(&self) -> Result<File, LaneError> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock)
            .map_err(|source| io(self.lock, source))
    }

    fn take(&self, request: &Request<'_>, progress: &mut dyn Write) -> Result<File, LaneError> {
        let marker = self.directory.join(format!(
            "{}.waiting.{}",
            request.lane.name(),
            std::process::id()
        ));
        let mut announced = false;
        let started = Instant::now();
        let mut reported = started;
        loop {
            let lock = self.open()?;
            match lock.try_lock() {
                Ok(()) if self.still_named(&lock)? => {
                    if announced {
                        std::fs::remove_file(&marker).map_err(|source| io(&marker, source))?;
                    }
                    return Ok(lock);
                }
                Ok(()) | Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(source)) => {
                    return Err(LaneError::Lock {
                        path: self.lock.display().to_string(),
                        source,
                    });
                }
            }
            drop(lock);
            if !announced {
                announced = true;
                self.forget_the_dead(request.lane)?;
                std::fs::write(&marker, &request.holder.command)
                    .map_err(|source| io(&marker, source))?;
                say(
                    progress,
                    &format!(
                        "slot: waiting for the {} lane, held by {}",
                        request.lane.name(),
                        describe(self.record)
                    ),
                )?;
            }
            if let Some(signal) = request.stops.raised() {
                std::fs::remove_file(&marker).map_err(|source| io(&marker, source))?;
                return Err(LaneError::Interrupted {
                    lane: request.lane.name(),
                    signal,
                });
            }
            if reported.elapsed() >= REPORT {
                reported = Instant::now();
                say(
                    progress,
                    &format!(
                        "slot: still waiting for the {} lane after {}, held by {}; load now {}",
                        request.lane.name(),
                        span(started.elapsed().as_secs()),
                        describe(self.record),
                        load()
                    ),
                )?;
            }
            std::thread::sleep(POLL);
        }
    }

    /// Whether the locked file is still the one the lane's path names, so a lock file somebody removed cannot let two runs in.
    #[cfg(unix)]
    fn still_named(&self, lock: &File) -> Result<bool, LaneError> {
        use std::os::unix::fs::MetadataExt as _;

        let held = lock.metadata().map_err(|source| io(self.lock, source))?;
        match std::fs::metadata(self.lock) {
            Ok(named) => Ok(named.dev() == held.dev() && named.ino() == held.ino()),
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(io(self.lock, source)),
        }
    }

    #[cfg(not(unix))]
    fn still_named(&self, _lock: &File) -> Result<bool, LaneError> {
        Ok(true)
    }

    /// Removes the waiting markers of runs that are no longer alive to wait.
    fn forget_the_dead(&self, lane: Lane) -> Result<(), LaneError> {
        if cfg!(not(unix)) {
            return Ok(());
        }
        let prefix = format!("{}.waiting.", lane.name());
        let entries =
            std::fs::read_dir(self.directory).map_err(|source| io(self.directory, source))?;
        for entry in entries {
            let entry = entry.map_err(|source| io(self.directory, source))?;
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|name| name.strip_prefix(prefix.as_str()))
                .and_then(number)
            else {
                continue;
            };
            if started_at(pid).is_none() {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => return Err(io(&entry.path(), source)),
                }
            }
        }
        Ok(())
    }

    /// Waits for the work the last holder started to end, when that holder died before its work did.
    fn outlast(&self, request: &Request<'_>, progress: &mut dyn Write) -> Result<(), LaneError> {
        let Some((pid, born)) = leader_of(self.record) else {
            return Ok(());
        };
        let started = Instant::now();
        let mut reported: Option<Instant> = None;
        while started_at(pid).as_deref() == Some(born.as_str()) {
            if let Some(signal) = request.stops.raised() {
                return Err(LaneError::Interrupted {
                    lane: request.lane.name(),
                    signal,
                });
            }
            if reported.is_none_or(|last| last.elapsed() >= REPORT) {
                reported = Some(Instant::now());
                say(
                    progress,
                    &format!(
                        "slot: the {} lane is free, but the work its last holder started (pid {pid}) is still running; waited {} so far",
                        request.lane.name(),
                        span(started.elapsed().as_secs())
                    ),
                )?;
            }
            std::thread::sleep(POLL);
        }
        Ok(())
    }
}

/// A lane this process holds; dropping it lets the next run in, which overwrites the record when it starts.
#[derive(Debug)]
#[must_use = "a lane is held only for as long as this value lives"]
pub struct Held {
    #[expect(
        dead_code,
        reason = "the file is held for what dropping it does: the operating system releases the lock"
    )]
    lock: Option<File>,
    record: Option<PathBuf>,
}

impl Held {
    /// Records the process that leads the work this lane admitted, so a holder that dies before its work cannot let the next run in over it.
    ///
    /// # Errors
    /// Returns the filesystem failure when the record cannot be rewritten.
    pub fn working_on(&self, leader: u32) -> std::io::Result<()> {
        let Some(record) = &self.record else {
            return Ok(());
        };
        let Some(born) = started_at(leader) else {
            return if cfg!(unix) {
                Err(std::io::Error::other(format!(
                    "when the work's leader (pid {leader}) started could not be read, so a holder \
                     killed outright could let the next run in over it"
                )))
            } else {
                Ok(())
            };
        };
        let text = std::fs::read_to_string(record)?;
        let mut kept: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with("leader="))
            .collect();
        let leading = format!("leader={leader} {born}");
        kept.push(&leading);
        kept.push("");
        replace(record, &kept.join("\n"))
    }
}

/// The value of `name` in `environment`, when it is set to something.
#[must_use]
pub fn variable<'a>(environment: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    environment
        .iter()
        .find(|(key, value)| key == name && !value.is_empty())
        .map(|(_key, value)| value.as_os_str())
}

/// The branch and short commit of the checkout at `directory`, or a dash for each that cannot be read; no `GIT_*` variable of `environment` reaches the git it asks.
#[must_use]
pub fn revision_of(directory: &Path, environment: &[(OsString, OsString)]) -> String {
    let ask = |arguments: &[&str]| {
        let mut git = Command::new("git");
        for (name, _value) in environment {
            if name.as_encoded_bytes().starts_with(b"GIT_") {
                git.env_remove(name);
            }
        }
        answer(git.args(arguments).current_dir(directory)).unwrap_or_else(|| "-".to_owned())
    };
    format!(
        "{} {}",
        ask(&["rev-parse", "--abbrev-ref", "HEAD"]),
        ask(&["rev-parse", "--short", "HEAD"])
    )
}

/// When the process `pid` started, as the operating system spells it, so a recycled pid is not taken for the process that had it.
#[cfg(target_os = "linux")]
fn started_at(pid: u32) -> Option<String> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(_gone) => return None,
    };
    let after_name = stat.rsplit_once(')')?.1;
    after_name.split_whitespace().nth(19).map(str::to_owned)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn started_at(pid: u32) -> Option<String> {
    answer(
        Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", "UTC0"),
    )
    .filter(|started| !started.is_empty())
}

#[cfg(not(unix))]
const fn started_at(_pid: u32) -> Option<String> {
    None
}

/// The leader the record names and when it started.
fn leader_of(record: &Path) -> Option<(u32, String)> {
    let text = match std::fs::read_to_string(record) {
        Ok(text) => text,
        Err(_no_record) => return None,
    };
    let line = text.lines().find_map(|line| line.strip_prefix("leader="))?;
    let (pid, born) = line.split_once(' ')?;
    Some((number(pid)?, born.to_owned()))
}

fn number(text: &str) -> Option<u32> {
    match text.parse::<u32>() {
        Ok(number) => Some(number),
        Err(_not_a_pid) => None,
    }
}

fn answer(command: &mut Command) -> Option<String> {
    let output = match command.output() {
        Ok(output) => output,
        Err(_absent) => return None,
    };
    if !output.status.success() {
        return None;
    }
    match String::from_utf8(output.stdout) {
        Ok(text) => Some(text.trim().to_owned()),
        Err(_not_text) => None,
    }
}

fn state_directory(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    if let Some(state) = variable(environment, "XDG_STATE_HOME") {
        return Some(PathBuf::from(state));
    }
    if let Some(home) = variable(environment, "HOME") {
        return Some(Path::new(home).join(".local").join("state"));
    }
    variable(environment, "LOCALAPPDATA").map(PathBuf::from)
}

fn record_of(holder: &Holder) -> String {
    format!(
        "pid={}\nsince={}\nworktree={}\nrevision={}\ncommand={}\nload={}\n",
        std::process::id(),
        now(),
        holder.worktree.display(),
        holder.revision,
        holder.command,
        load()
    )
}

fn describe(record: &Path) -> String {
    let text = match std::fs::read_to_string(record) {
        Ok(text) => text,
        Err(_unwritten) => return "a run that has not written its record yet".to_owned(),
    };
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .unwrap_or("?")
            .to_owned()
    };
    let held_for = match field("since").parse::<u64>() {
        Ok(since) => span(now().saturating_sub(since)),
        Err(_unreadable) => "?".to_owned(),
    };
    format!(
        "pid {} in {} ({}) for {}: {}; load then {}",
        field("pid"),
        field("worktree"),
        field("revision"),
        held_for,
        field("command"),
        field("load")
    )
}

fn now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs(),
        Err(_before_the_epoch) => 0,
    }
}

fn span(seconds: u64) -> String {
    let minutes = seconds.checked_div(60).unwrap_or_default();
    let rest = seconds.checked_rem(60).unwrap_or_default();
    if minutes == 0 {
        format!("{rest}s")
    } else {
        format!("{minutes}m{rest:02}s")
    }
}

fn load() -> String {
    answer(&mut Command::new("uptime"))
        .and_then(|text| averages(&text))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn averages(uptime: &str) -> Option<String> {
    let after = uptime.split_once("load average")?.1;
    Some(after.split_once(':')?.1.trim().to_owned())
}

/// Writes `text` beside `path` and renames it over, so a reader never sees a record half written.
fn replace(path: &Path, text: &str) -> std::io::Result<()> {
    let written = path.with_extension("next");
    std::fs::write(&written, text)?;
    std::fs::rename(&written, path)
}

fn say(progress: &mut dyn Write, line: &str) -> Result<(), LaneError> {
    writeln!(progress, "{line}").map_err(|source| LaneError::Progress { source })
}

fn io(path: &Path, source: std::io::Error) -> LaneError {
    LaneError::Io {
        path: path.display().to_string(),
        source,
    }
}
