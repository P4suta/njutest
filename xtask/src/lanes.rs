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

/// The variable that names the lanes a process already holds, so a run inside one never waits for itself.
pub const HELD: &str = "NJUTEST_SLOT_HELD";

/// How often a waiting run looks at the lock again.
const POLL: Duration = Duration::from_millis(200);

/// How often a waiting run repeats whom it is waiting for.
const REPORT: Duration = Duration::from_secs(30);

/// A lane a run can queue for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// A run that compiles or tests the whole workspace.
    Heavy,
}

impl Lane {
    /// The lane's name on the command line, in its files, and in [`HELD`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Heavy => "heavy",
        }
    }

    /// The lane `name` spells, if it spells one.
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
}

/// Where this machine keeps its lanes, and which of them the running process already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lanes {
    directory: PathBuf,
    held: Vec<String>,
}

impl Lanes {
    /// The lanes the environment names: `NJUTEST_SLOT_DIR`, else under the state directory, with [`HELD`] saying which are already held.
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

    /// The value of [`HELD`] for a child of a process that holds `lane`.
    #[must_use]
    pub fn held_with(&self, lane: Lane) -> String {
        let mut held = self.held.clone();
        if !held.iter().any(|name| name == lane.name()) {
            held.push(lane.name().to_owned());
        }
        held.join(",")
    }

    /// Waits until `lane` is free, telling `progress` whom it waits for, and holds it until the answer is dropped.
    ///
    /// # Errors
    /// Returns a [`LaneError`] when the lane's files cannot be written or its lock cannot be taken at all.
    pub fn hold(
        &self,
        lane: Lane,
        holder: &Holder,
        progress: &mut dyn Write,
    ) -> Result<Held, LaneError> {
        if self.held.iter().any(|name| name == lane.name()) {
            return Ok(Held { lock: None });
        }
        std::fs::create_dir_all(&self.directory).map_err(|source| io(&self.directory, source))?;
        let lock_path = self.directory.join(format!("{}.lock", lane.name()));
        let record = self.directory.join(format!("{}.holder", lane.name()));
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&lock_path)
            .map_err(|source| io(&lock_path, source))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let queue = Queue {
                    lane,
                    lock: &lock,
                    record: &record,
                };
                self.wait(&queue, holder, progress)?;
            }
            Err(TryLockError::Error(source)) => {
                return Err(LaneError::Lock {
                    path: lock_path.display().to_string(),
                    source,
                });
            }
        }
        std::fs::write(&record, record_of(holder)).map_err(|source| io(&record, source))?;
        Ok(Held { lock: Some(lock) })
    }

    fn wait(
        &self,
        queue: &Queue<'_>,
        holder: &Holder,
        progress: &mut dyn Write,
    ) -> Result<(), LaneError> {
        let Queue { lane, lock, record } = *queue;
        let marker = self
            .directory
            .join(format!("{}.waiting.{}", lane.name(), std::process::id()));
        std::fs::write(&marker, &holder.command).map_err(|source| io(&marker, source))?;
        say(
            progress,
            &format!(
                "slot: waiting for the {} lane, held by {}",
                lane.name(),
                describe(record)
            ),
        )?;
        let started = Instant::now();
        let mut reported = started;
        let taken = loop {
            std::thread::sleep(POLL);
            match lock.try_lock() {
                Ok(()) => break Ok(()),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(source)) => {
                    break Err(LaneError::Lock {
                        path: record.display().to_string(),
                        source,
                    });
                }
            }
            if reported.elapsed() >= REPORT {
                reported = Instant::now();
                say(
                    progress,
                    &format!(
                        "slot: still waiting for the {} lane after {}, held by {}; load now {}",
                        lane.name(),
                        span(started.elapsed().as_secs()),
                        describe(record),
                        load()
                    ),
                )?;
            }
        };
        std::fs::remove_file(&marker).map_err(|source| io(&marker, source))?;
        taken
    }
}

/// A lane another run holds, as the run waiting for it sees it.
#[derive(Debug, Clone, Copy)]
struct Queue<'a> {
    lane: Lane,
    lock: &'a File,
    record: &'a Path,
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
}

/// The value of `name` in `environment`, when it is set to something.
#[must_use]
pub fn variable<'a>(environment: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    environment
        .iter()
        .find(|(key, value)| key == name && !value.is_empty())
        .map(|(_key, value)| value.as_os_str())
}

/// The branch and short commit of the checkout at `directory`, or a dash for each that cannot be read.
#[must_use]
pub fn revision_of(directory: &Path) -> String {
    let ask = |arguments: &[&str]| {
        answer(Command::new("git").args(arguments).current_dir(directory))
            .unwrap_or_else(|| "-".to_owned())
    };
    format!(
        "{} {}",
        ask(&["rev-parse", "--abbrev-ref", "HEAD"]),
        ask(&["rev-parse", "--short", "HEAD"])
    )
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

fn say(progress: &mut dyn Write, line: &str) -> Result<(), LaneError> {
    writeln!(progress, "{line}").map_err(|source| LaneError::Progress { source })
}

fn io(path: &Path, source: std::io::Error) -> LaneError {
    LaneError::Io {
        path: path.display().to_string(),
        source,
    }
}
