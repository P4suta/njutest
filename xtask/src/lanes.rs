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

/// How long work a dead holder left behind is given to end once asked, and again once killed.
#[cfg(unix)]
const ORPHAN_GRACE: Duration = Duration::from_secs(10);

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
    /// The work a dead holder left running in the lane would not end when this run stopped it.
    #[cfg(unix)]
    #[error(
        "the {lane} lane's last holder is gone and a process of the group its work ran in (led by \
         pid {pid}) outlived both the request to stop and the kill; nothing else will take the \
         lane while it runs"
    )]
    Unended {
        /// The lane.
        lane: &'static str,
        /// The group's leader, whose id the group carries.
        pid: u32,
    },
    /// Whether the holder the record names still runs could not be read.
    #[cfg(unix)]
    #[error(
        "the {lane} lane's lock was free, and whether its recorded holder (pid {pid}) still runs \
         could not be read, so its work is not ended and the lane is not taken over it"
    )]
    HolderUnseen {
        /// The lane.
        lane: &'static str,
        /// The holder.
        pid: u32,
    },
    /// Whether the group a dead holder's work ran in is still there could not be seen.
    #[cfg(unix)]
    #[error(
        "the {lane} lane's last holder is gone and the processes of the group its work ran in (led \
         by pid {pid}) could not be listed, so whether that work has ended is unknown and the lane \
         is not taken over it"
    )]
    Unseen {
        /// The lane.
        lane: &'static str,
        /// The group's leader.
        pid: u32,
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
            #[cfg(unix)]
            Self::Unended { .. } | Self::Unseen { .. } | Self::HolderUnseen { .. } => {
                crate::error::XtCode::LaneUnavailable
            }
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
            #[cfg(unix)]
            Self::Unended { .. } | Self::Unseen { .. } | Self::HolderUnseen { .. } => None,
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

    /// The lane this process is already inside, to record the work it starts there, or nothing when it holds none.
    #[must_use]
    pub fn inside(&self, lane: Lane) -> Option<Held> {
        self.held
            .iter()
            .any(|name| name == lane.name())
            .then(|| Held {
                lock: None,
                record: Some(self.directory.join(format!("{}.holder", lane.name()))),
                unended: std::cell::Cell::new(false),
            })
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
            std::fs::create_dir_all(&self.directory)
                .map_err(|source| io(&self.directory, source))?;
            return Ok(Held {
                lock: None,
                record: Some(self.directory.join(format!("{}.holder", lane.name()))),
                unended: std::cell::Cell::new(false),
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
            unended: std::cell::Cell::new(false),
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
        let ticket = self.queue(request, &marker)?;
        let mut announced = false;
        let started = Instant::now();
        let mut reported = started;
        loop {
            if self.first_in_line(request.lane, ticket)? {
                let lock = self.open()?;
                match lock.try_lock() {
                    Ok(()) if self.still_named(&lock)? => {
                        std::fs::remove_file(&marker).map_err(|source| io(&marker, source))?;
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
            }
            if !announced {
                announced = true;
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
    #[expect(
        clippy::missing_const_for_fn,
        clippy::unnecessary_wraps,
        clippy::unused_self,
        reason = "only unix can tell the lock this run holds from a file now at its path, so elsewhere the answer is yes, in the signature the unix check needs"
    )]
    fn still_named(&self, _lock: &File) -> Result<bool, LaneError> {
        Ok(true)
    }

    /// Takes this run's place in the lane's line: a marker naming it, with a ticket after every one a live run already holds.
    fn queue(&self, request: &Request<'_>, marker: &Path) -> Result<u64, LaneError> {
        let line = self.directory.join(format!("{}.line", request.lane.name()));
        let turn = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&line)
            .map_err(|source| io(&line, source))?;
        turn.lock().map_err(|source| LaneError::Lock {
            path: line.display().to_string(),
            source,
        })?;
        let ticket = match self
            .waiting(request.lane)?
            .iter()
            .map(|waiter| waiter.ticket)
            .max()
        {
            Some(last) => last.saturating_add(1),
            None => 0,
        };
        let born = started_at(std::process::id()).unwrap_or_default();
        std::fs::write(
            marker,
            format!(
                "ticket={ticket}\nborn={born}\ncommand={}\n",
                request.holder.command
            ),
        )
        .map_err(|source| io(marker, source))?;
        drop(turn);
        Ok(ticket)
    }

    /// Whether no live run holds an earlier ticket than `ticket`, which only a machine that can tell a live run from a dead one can answer; elsewhere every run is first and the lock alone decides.
    fn first_in_line(&self, lane: Lane, ticket: u64) -> Result<bool, LaneError> {
        if cfg!(not(unix)) {
            return Ok(true);
        }
        let me = std::process::id();
        Ok(!self
            .waiting(lane)?
            .iter()
            .any(|waiter| waiter.pid != me && (waiter.ticket, waiter.pid) < (ticket, me)))
    }

    /// Every run still alive to wait for `lane`, with its ticket, after removing the markers of runs that are not.
    /// A marker from before tickets holds only its command, and its run is taken to have waited longest.
    fn waiting(&self, lane: Lane) -> Result<Vec<Waiter>, LaneError> {
        let prefix = format!("{}.waiting.", lane.name());
        let entries = crate::repository::entries(self.directory)
            .map_err(|source| io(self.directory, source))?;
        let mut alive = Vec::new();
        for entry in entries {
            let Some(pid) = entry
                .file_name()
                .and_then(OsStr::to_str)
                .and_then(|name| name.strip_prefix(prefix.as_str()))
                .and_then(number)
            else {
                continue;
            };
            let text = match std::fs::read_to_string(&entry) {
                Ok(text) => text,
                Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(io(&entry, source)),
            };
            let field = |name: &str| {
                text.lines()
                    .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            };
            let started = started_at(pid);
            let living = match (started.as_deref(), field("born")) {
                (None, _) => cfg!(not(unix)),
                (Some(_), None | Some("")) => true,
                (Some(now), Some(born)) => now == born,
            };
            if !living {
                match std::fs::remove_file(&entry) {
                    Ok(()) => {}
                    Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => return Err(io(&entry, source)),
                }
                continue;
            }
            let ticket = match field("ticket").map(str::parse::<u64>) {
                Some(Ok(ticket)) => ticket,
                Some(Err(_)) | None => 0,
            };
            alive.push(Waiter { pid, ticket });
        }
        Ok(alive)
    }

    /// Ends every group the last holder's work ran in, when that holder died before its work did: each is asked to stop, then killed, and the lane is taken only once every one is seen gone.
    #[cfg(unix)]
    fn outlast(&self, request: &Request<'_>, progress: &mut dyn Write) -> Result<(), LaneError> {
        let text = match std::fs::read_to_string(self.record) {
            Ok(text) => text,
            Err(_no_record) => return Ok(()),
        };
        let Some((pid, born)) = text
            .lines()
            .find_map(|line| line.strip_prefix("pid="))
            .and_then(number)
            .zip(
                text.lines()
                    .find_map(|line| line.strip_prefix("holder_born="))
                    .filter(|born| !born.is_empty()),
            )
        else {
            return Ok(());
        };
        let groups = groups_of(&text, boot().as_deref());
        if groups.is_empty() {
            return Ok(());
        }
        Self::outwait_holder(request, progress, pid, born)?;
        for group in groups {
            self.end_group(request, progress, &group)?;
        }
        Ok(())
    }

    /// Nothing is recorded where no start time can be read, so there is no group a dead holder left to end.
    #[cfg(not(unix))]
    #[expect(
        clippy::unnecessary_wraps,
        clippy::unused_self,
        reason = "the same signature as the platform that records the groups it ends"
    )]
    const fn outlast(
        &self,
        _request: &Request<'_>,
        _progress: &mut dyn Write,
    ) -> Result<(), LaneError> {
        Ok(())
    }

    /// Waits while the holder the record names still runs, which a lock taken over a removed lock file cannot tell from one that died.
    #[cfg(unix)]
    fn outwait_holder(
        request: &Request<'_>,
        progress: &mut dyn Write,
        pid: u32,
        born: &str,
    ) -> Result<(), LaneError> {
        let lane = request.lane.name();
        let mut reported: Option<Instant> = None;
        loop {
            match holder_state(&start_of(pid), born) {
                HolderState::Dead => return Ok(()),
                HolderState::Unseen => return Err(LaneError::HolderUnseen { lane, pid }),
                HolderState::Alive => {}
            }
            if let Some(signal) = request.stops.raised() {
                return Err(LaneError::Interrupted { lane, signal });
            }
            if reported.is_none_or(|last| last.elapsed() >= REPORT) {
                reported = Some(Instant::now());
                say(
                    progress,
                    &format!(
                        "slot: the {lane} lane's lock was free but its holder (pid {pid}) still runs, as it does when the lock file was removed under it; waiting for it rather than ending its work"
                    ),
                )?;
            }
            std::thread::sleep(POLL);
        }
    }

    /// Asks the group `pid` led to stop, then kills it, until a look at it finds nobody that has not ended.
    #[cfg(unix)]
    fn end_group(
        &self,
        request: &Request<'_>,
        progress: &mut dyn Write,
        group: &Recorded,
    ) -> Result<(), LaneError> {
        let lane = request.lane.name();
        let pid = group.pid;
        for sent in crate::work::Sent::ALL {
            match liveness(group) {
                Liveness::Gone => return Ok(()),
                Liveness::Unseen => return Err(LaneError::Unseen { lane, pid }),
                Liveness::Alive => {}
            }
            say(
                progress,
                &format!(
                    "slot: the {lane} lane is free, but the group its last holder's work ran in (led by pid {pid}) still holds a process with nobody to answer to; {} it",
                    match sent {
                        crate::work::Sent::Ask => "asking it to stop",
                        crate::work::Sent::Kill => "it did not stop when asked, so killing",
                    }
                ),
            )?;
            stop_group(pid, sent).map_err(|source| io(self.record, source))?;
            let asked = Instant::now();
            while liveness(group) == Liveness::Alive && asked.elapsed() < ORPHAN_GRACE {
                if let Some(signal) = request.stops.raised() {
                    return Err(LaneError::Interrupted { lane, signal });
                }
                std::thread::sleep(POLL);
            }
        }
        match liveness(group) {
            Liveness::Gone => Ok(()),
            Liveness::Alive => Err(LaneError::Unended { lane, pid }),
            Liveness::Unseen => Err(LaneError::Unseen { lane, pid }),
        }
    }
}

/// Whether a group a lane's work ran in still holds a process that has not ended.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// It does.
    Alive,
    /// It does not.
    Gone,
    /// The processes could not be listed, which answers neither.
    Unseen,
}

/// Whether the holder a lane's record names still runs.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderState {
    /// It runs since the recorded time.
    Alive,
    /// It is gone, or its id names a process started at another time.
    Dead,
    /// Whether it runs could not be read.
    Unseen,
}

/// Whether the holder recorded as started at `born` still runs, from its start as the machine answers now.
#[cfg(unix)]
#[must_use]
pub fn holder_state(now: &Start, born: &str) -> HolderState {
    match now {
        Start::Running(started) if started == born => HolderState::Alive,
        Start::Running(_) | Start::Absent => HolderState::Dead,
        Start::Unread => HolderState::Unseen,
    }
}

/// A group a lane's record names: its leader's id, when the leader started, the session it ran in, and the holder it ran under.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The leader's id, which the group carries.
    pub pid: u32,
    /// When the leader started, as a lane records it.
    pub born: String,
    /// The session the leader ran in, when it could be read.
    pub session: Option<u32>,
}

/// When a process started, as far as this machine can say.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// It runs, and started then.
    Running(String),
    /// Nothing has that id.
    Absent,
    /// Whether anything has it could not be read.
    Unread,
}

/// The session a process belongs to, as far as this machine can say.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// This one.
    Of(u32),
    /// The process is gone.
    Gone,
    /// It could not be read.
    Unread,
}

/// Whether the group `recorded` names still holds a process of its work that has not ended.
///
/// A leader running since the recorded time makes every member of its group the work's; one started at another time means the id is somebody else's.
/// Once the leader is gone, a group by that id is the work's only where a member is in the session the leader was, since a group whose id came round again belongs to whoever reused it; a record with no session names nothing then.
/// A start or a session that could not be read answers neither way, and a zombie has ended.
#[cfg(unix)]
#[must_use]
pub fn group_liveness(
    recorded: &Recorded,
    leader: &Start,
    listed: Option<&[crate::work::Listed]>,
    session_of: impl Fn(u32) -> Session,
) -> Liveness {
    let tied = match leader {
        Start::Running(now) if *now != recorded.born => return Liveness::Gone,
        Start::Running(_) => None,
        Start::Unread => return Liveness::Unseen,
        Start::Absent => match recorded.session {
            Some(session) => Some(session),
            None => return Liveness::Gone,
        },
    };
    let Some(processes) = listed else {
        return Liveness::Unseen;
    };
    let mut unread = false;
    for member in processes
        .iter()
        .filter(|one| one.group == recorded.pid && !one.ended)
    {
        let Some(session) = tied else {
            return Liveness::Alive;
        };
        match session_of(member.pid) {
            Session::Of(its) if its == session => return Liveness::Alive,
            Session::Unread => unread = true,
            Session::Of(_) | Session::Gone => {}
        }
    }
    if unread {
        Liveness::Unseen
    } else {
        Liveness::Gone
    }
}

/// Whether the group `recorded` names still holds a process of its work that has not ended, as the machine answers now.
#[cfg(unix)]
fn liveness(recorded: &Recorded) -> Liveness {
    group_liveness(
        recorded,
        &start_of(recorded.pid),
        crate::work::listed().as_deref(),
        session_of,
    )
}

/// Every group a lane's record names under the holder that wrote it, or none when that holder let the lane go itself or the record was written in another boot; a line the record ends in without its newline is one still being written, and is not read.
#[cfg(unix)]
#[must_use]
pub fn groups_of(record: &str, this_boot: Option<&str>) -> Vec<Recorded> {
    let written_in = record
        .lines()
        .find_map(|line| line.strip_prefix("boot="))
        .filter(|then| !then.is_empty());
    if let (Some(then), Some(now)) = (written_in, this_boot)
        && then != now
    {
        return Vec::new();
    }
    if record.lines().any(|line| line == "released") {
        return Vec::new();
    }
    let holder = record.lines().find_map(|line| line.strip_prefix("pid="));
    record
        .split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
        .filter_map(|line| {
            line.strip_prefix("group=")
                .or_else(|| line.strip_prefix("leader="))
        })
        .filter_map(|group| recorded(group, holder))
        .collect()
}

/// One group line, written as `pid holder=H session=S born=B` or, before sessions were recorded, as `pid B`, or nothing when it names another holder.
#[cfg(unix)]
fn recorded(line: &str, holder: Option<&str>) -> Option<Recorded> {
    let (pid, rest) = line.split_once(' ')?;
    let pid = number(pid)?;
    let Some(tagged) = rest.strip_prefix("holder=") else {
        return Some(Recorded {
            pid,
            born: rest.to_owned(),
            session: None,
        });
    };
    let (under, rest) = tagged.split_once(' ')?;
    if holder.is_some_and(|holder| holder != under) {
        return None;
    }
    let (session, born) = rest.strip_prefix("session=")?.split_once(" born=")?;
    Some(Recorded {
        pid,
        born: born.to_owned(),
        session: number(session),
    })
}

/// What names this boot of the machine: the kernel's boot id on Linux, and when it booted on macOS.
#[must_use]
pub fn boot() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
            Ok(id) => Some(id.trim().to_owned()),
            Err(_unreadable) => None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        answer(Command::new("sysctl").args(["-n", "kern.boottime"]))
            .and_then(|said| said.split(',').next().map(str::to_owned))
            .filter(|seconds| seconds.contains("sec"))
    }
}

/// Stops the group led by `pid`, whether or not its leader is still alive.
#[cfg(unix)]
fn stop_group(pid: u32, sent: crate::work::Sent) -> std::io::Result<()> {
    let unled = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{pid} is not a process id a group can be led by"),
        )
    };
    let raw = match i32::try_from(pid) {
        Ok(raw) => raw,
        Err(_wider_than_a_pid) => return Err(unled()),
    };
    let leader = rustix::process::Pid::from_raw(raw).ok_or_else(unled)?;
    match crate::work::signal_group(leader, sent) {
        Ok(()) | Err(crate::work::WorkError::Outlived) => Ok(()),
        Err(error) => Err(std::io::Error::other(error.to_string())),
    }
}

/// A run waiting for a lane, and its place in the line.
#[derive(Debug, Clone, Copy)]
struct Waiter {
    pid: u32,
    ticket: u64,
}

/// A lane this process holds; dropping it lets the next run in, which overwrites the record when it starts.
#[derive(Debug)]
#[must_use = "a lane is held only for as long as this value lives"]
pub struct Held {
    lock: Option<File>,
    record: Option<PathBuf>,
    unended: std::cell::Cell<bool>,
}

impl Drop for Held {
    /// Writes into the record that this holder let the lane go itself, before the lock goes: what its work left in its groups — a compilation cache's server, Git's file monitor — is somebody's to keep, and the next run leaves it; a holder that dies never writes it, and its groups are ended.
    fn drop(&mut self) {
        if self.lock.is_some()
            && !self.unended.get()
            && let Some(record) = &self.record
        {
            match OpenOptions::new()
                .append(true)
                .open(record)
                .and_then(|mut appending| appending.write_all(b"released\n"))
            {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

impl Held {
    /// Says this holder's work could not be stopped, so the lane is let go without `released` and the next run ends what is left of it.
    pub fn left_work_running(&self) {
        self.unended.set(true);
    }

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
        let holder = match std::fs::read_to_string(record) {
            Ok(text) => text
                .lines()
                .find_map(|line| line.strip_prefix("pid="))
                .unwrap_or("")
                .to_owned(),
            Err(_no_record_yet) => String::new(),
        };
        let session = session_text(leader);
        let mut appending = OpenOptions::new().create(true).append(true).open(record)?;
        appending.write_all(
            format!("group={leader} holder={holder} session={session} born={born}\n").as_bytes(),
        )
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

/// When the process `pid` started, as a lane records it.
#[cfg(all(unix, feature = "testkit"))]
#[must_use]
pub fn started(pid: u32) -> Option<String> {
    started_at(pid)
}

/// When the process `pid` started, as a lane records it.
#[cfg(all(not(unix), feature = "testkit"))]
#[must_use]
pub const fn started(pid: u32) -> Option<String> {
    started_at(pid)
}

/// The session the process `pid` belongs to, as a lane records it.
#[cfg(all(unix, feature = "testkit"))]
#[must_use]
pub fn session(pid: u32) -> Option<u32> {
    match session_of(pid) {
        Session::Of(session) => Some(session),
        Session::Gone | Session::Unread => None,
    }
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

/// When the process `pid` started, and whether it runs at all, telling a process that is gone from one that could not be read.
#[cfg(target_os = "linux")]
fn start_of(pid: u32) -> Start {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => return Start::Absent,
        Err(_unreadable) => return Start::Unread,
    };
    match stat
        .rsplit_once(')')
        .and_then(|(_, after)| after.split_whitespace().nth(19))
    {
        Some(started) => Start::Running(started.to_owned()),
        None => Start::Unread,
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn start_of(pid: u32) -> Start {
    let output = match Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC0")
        .output()
    {
        Ok(output) => output,
        Err(_no_ps) => return Start::Unread,
    };
    let said = match String::from_utf8(output.stdout) {
        Ok(said) => said.trim().to_owned(),
        Err(_not_text) => return Start::Unread,
    };
    match (output.status.success(), said.is_empty()) {
        (true, false) => Start::Running(said),
        (false, true) => Start::Absent,
        (true, true) | (false, false) => Start::Unread,
    }
}

/// The session the process `pid` belongs to, as a lane's record spells it, or nothing where it cannot be read.
#[cfg(unix)]
fn session_text(pid: u32) -> String {
    match session_of(pid) {
        Session::Of(session) => session.to_string(),
        Session::Gone | Session::Unread => String::new(),
    }
}

#[cfg(not(unix))]
const fn session_text(_pid: u32) -> String {
    String::new()
}

/// The session the process `pid` belongs to.
#[cfg(unix)]
fn session_of(pid: u32) -> Session {
    let Some(pid) = (match i32::try_from(pid) {
        Ok(raw) => rustix::process::Pid::from_raw(raw),
        Err(_beyond_a_pid) => None,
    }) else {
        return Session::Unread;
    };
    match rustix::process::getsid(Some(pid)) {
        Ok(session) => match u32::try_from(session.as_raw_nonzero().get()) {
            Ok(session) => Session::Of(session),
            Err(_negative) => Session::Unread,
        },
        Err(rustix::io::Errno::SRCH) => Session::Gone,
        Err(_unreadable) => Session::Unread,
    }
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
        "pid={}\nholder_born={}\nsince={}\nworktree={}\nrevision={}\ncommand={}\nload={}\nboot={}\n",
        std::process::id(),
        started_at(std::process::id()).unwrap_or_default(),
        now(),
        holder.worktree.display(),
        holder.revision,
        holder.command,
        load(),
        boot().unwrap_or_default()
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
