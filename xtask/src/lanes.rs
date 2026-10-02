// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Machine-wide lanes that admit one whole-workspace run at a time, and say who holds each.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::environment::Environment;
use crate::observation::{Event, Observation};
use crate::work::Stops;

/// The variable that names the lanes a process already holds, so a run inside one never waits for itself.
pub const HELD: &str = "NJUTEST_SLOT_HELD";

/// The variable that says, in seconds, how long a run waits behind a holder whose work shows nothing new.
pub const QUIET: &str = "NJUTEST_SLOT_QUIET_SECONDS";

/// How long a run waits behind a holder whose work shows nothing new, where [`QUIET`] does not say.
const DEFAULT_QUIET: Duration = Duration::from_secs(600);

/// The minimum interval between actual operating-system CPU samples.
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
    /// A legacy receipt still names a listed group with no retained settlement capability.
    #[cfg(unix)]
    #[error(
        "the {lane} lane's legacy group (led by pid {pid}) still has a listed member, but its \
         receipt retains no complete group and descriptor ownership; settlement is unprovable \
         and the lane is not taken over it"
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
    /// Whether a run waiting for the lane still exists could not be read.
    #[cfg(unix)]
    #[error(
        "whether the run waiting for the {lane} lane (pid {pid}) still exists could not be read, \
         so its place in line is not removed"
    )]
    WaiterUnseen {
        /// The lane.
        lane: &'static str,
        /// The waiting run.
        pid: u32,
    },
    /// The recorded generation lacks a capability proving its whole group and descriptors ended.
    #[cfg(unix)]
    #[error(
        "the {lane} lane's legacy group receipt (led by pid {pid}) retains no ownership of its \
         complete group and descriptors, so settlement is unprovable and the lane is not taken \
         over it"
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
    /// A setting of the lanes is not a whole number of seconds.
    #[error("{name} is {value:?}, which is not a whole number of seconds")]
    Setting {
        /// The variable.
        name: &'static str,
        /// What it holds.
        value: String,
    },
    /// The holder showed nothing new of its work for longer than the quiet window allows.
    #[error(
        "the {lane} lane's holder, {holder}, has shown nothing new of its work for {silent}: its \
         record and every process of the groups it names have neither changed nor used the \
         processor for as long as {QUIET} allows ({quiet}), so this run stops waiting rather than \
         wait for it forever"
    )]
    Stalled {
        /// The lane.
        lane: &'static str,
        /// The holder, as its record describes it.
        holder: String,
        /// How long it showed nothing new.
        silent: String,
        /// The window.
        quiet: String,
    },
}

impl crate::error::Coded for LaneError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::NotText { .. }
            | Self::Nowhere
            | Self::Io { .. }
            | Self::Lock { .. }
            | Self::Progress { .. }
            | Self::Setting { .. } => crate::error::XtCode::LaneUnavailable,
            #[cfg(unix)]
            Self::Unended { .. }
            | Self::Unseen { .. }
            | Self::HolderUnseen { .. }
            | Self::WaiterUnseen { .. } => crate::error::XtCode::LaneUnavailable,
            Self::Stalled { .. } => crate::error::XtCode::LaneStalled,
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
            | Self::Progress { .. }
            | Self::Setting { .. }
            | Self::Stalled { .. } => None,
            #[cfg(unix)]
            Self::Unended { .. }
            | Self::Unseen { .. }
            | Self::HolderUnseen { .. }
            | Self::WaiterUnseen { .. } => None,
        }
    }
}

/// Where lanes are kept, which of them the running process already holds, and how long a run waits behind a holder whose work shows nothing new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lanes {
    directory: PathBuf,
    held: Vec<String>,
    quiet: Duration,
}

impl Lanes {
    /// The machine's lanes as the environment names them: `NJUTEST_SLOT_DIR`, else under the state directory, with [`HELD`] saying which are already held and [`QUIET`] how long a waiting run bears a holder that shows nothing new.
    ///
    /// # Errors
    /// Returns [`LaneError::Nowhere`] when the environment names no directory for them, and [`LaneError::Setting`] when [`QUIET`] is not a whole number of seconds.
    pub fn from_environment(environment: &Environment) -> Result<Self, LaneError> {
        let directory = match environment.value("NJUTEST_SLOT_DIR") {
            Some(named) => PathBuf::from(named),
            None => state_directory(environment)
                .ok_or(LaneError::Nowhere)?
                .join("njutest")
                .join("slots"),
        };
        let held = match environment.value(HELD) {
            Some(value) => value
                .to_str()
                .ok_or(LaneError::NotText { name: HELD })?
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        let quiet = match environment.value(QUIET) {
            Some(value) => {
                let text = value.to_str().ok_or(LaneError::NotText { name: QUIET })?;
                match text.trim().parse::<u64>() {
                    Ok(seconds) => Duration::from_secs(seconds),
                    Err(_not_seconds) => {
                        return Err(LaneError::Setting {
                            name: QUIET,
                            value: text.to_owned(),
                        });
                    }
                }
            }
            None => DEFAULT_QUIET,
        };
        Ok(Self {
            directory,
            held,
            quiet,
        })
    }

    /// Lanes kept in `directory` that nothing already holds, such as one repository's gate tree, waited for as long as their holder shows something new within `quiet`.
    #[must_use]
    pub const fn at(directory: PathBuf, quiet: Duration) -> Self {
        Self {
            directory,
            held: Vec::new(),
            quiet,
        }
    }

    /// How long a run waits behind a holder whose work shows nothing new.
    #[must_use]
    pub const fn quiet(&self) -> Duration {
        self.quiet
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
    /// Returns a [`LaneError`] when the lane's files cannot be written, its lock cannot be taken, a signal ends the wait, or its holder shows nothing new of its work for as long as the quiet window.
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
            quiet: self.quiet,
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

/// The files one lane lives in, and how long a run waits there behind a holder that shows nothing new.
#[derive(Debug, Clone, Copy)]
struct Place<'a> {
    directory: &'a Path,
    lock: &'a Path,
    record: &'a Path,
    quiet: Duration,
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
        let observed = Observation::filesystem(self.directory, false)
            .map_err(|source| io(self.directory, source))?;
        let stopping = request.stops.subscribe(&observed);
        let taken = self.take_observed(request, progress, &observed);
        drop(stopping);
        taken
    }

    fn take_observed(
        &self,
        request: &Request<'_>,
        progress: &mut dyn Write,
        observed: &Observation,
    ) -> Result<File, LaneError> {
        let marker = self.directory.join(format!(
            "{}.waiting.{}",
            request.lane.name(),
            std::process::id()
        ));
        let ticket = self.queue(request, &marker)?;
        let mut announced = false;
        let started = Instant::now();
        let mut reported = started;
        let mut watch = Watch::new(self.quiet);
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
            if let Some(silent) = watch.stalled(self.record) {
                std::fs::remove_file(&marker).map_err(|source| io(&marker, source))?;
                return Err(self.stalled(request, silent));
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
            let deadline = watch
                .deadline(reported)
                .map_err(|source| io(self.record, source))?;
            self.wait(observed, request, deadline)?;
        }
    }

    fn wait(
        &self,
        observed: &Observation,
        request: &Request<'_>,
        deadline: Instant,
    ) -> Result<(), LaneError> {
        let waited = observed
            .wait(
                &self.record.display().to_string(),
                "lane publication, cancellation, CPU sample or quiet deadline",
                Some(deadline),
            )
            .map_err(|source| io(self.record, source))?;
        request.stops.record(waited.note);
        match waited.event.map_err(|source| io(self.record, source))? {
            Event::Changed | Event::Completed | Event::Cancelled | Event::Deadline => Ok(()),
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
        #[cfg(unix)]
        let me = std::process::id();
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
            #[cfg(unix)]
            {
                let now = if pid == me {
                    Start::Unread
                } else {
                    start_of(pid)
                };
                match waiter_state(me, pid, field("born"), &now) {
                    HolderState::Alive => {}
                    HolderState::Dead => {
                        match std::fs::remove_file(&entry) {
                            Ok(()) => {}
                            Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => {}
                            Err(source) => return Err(io(&entry, source)),
                        }
                        continue;
                    }
                    HolderState::Unseen => {
                        return Err(LaneError::WaiterUnseen {
                            lane: lane.name(),
                            pid,
                        });
                    }
                }
            }
            let ticket = match field("ticket").map(str::parse::<u64>) {
                Some(Ok(ticket)) => ticket,
                Some(Err(_)) | None => 0,
            };
            alive.push(Waiter { pid, ticket });
        }
        Ok(alive)
    }

    /// Refuses unreleased legacy work whose complete group and descriptor lifetime was not retained.
    #[cfg(unix)]
    fn outlast(&self, request: &Request<'_>, progress: &mut dyn Write) -> Result<(), LaneError> {
        let observed = Observation::filesystem(self.directory, false)
            .map_err(|source| io(self.directory, source))?;
        let stopping = request.stops.subscribe(&observed);
        let bytes = match std::fs::read(self.record) {
            Ok(bytes) => bytes,
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(io(self.record, source)),
        };
        let written = Record::read(&bytes);
        for unread in written.unread() {
            say(
                progress,
                &format!(
                    "slot: the {} lane's record holds `{unread}`, which is no line its writer finished, so nothing it named is ended",
                    request.lane.name()
                ),
            )?;
        }
        let Some((pid, born)) = written
            .field("pid")
            .and_then(number)
            .zip(written.field("holder_born").filter(|born| !born.is_empty()))
        else {
            return Ok(());
        };
        let groups = written.unreleased(boot().as_deref());
        if groups.is_empty() {
            return Ok(());
        }
        self.outwait_holder(request, progress, (&observed, (pid, born)))?;
        drop(stopping);
        match groups.first() {
            Some(group) => {
                let lane = request.lane.name();
                let pid = group.pid;
                match group_liveness(
                    group,
                    &start_of(pid),
                    crate::work::listed().as_deref(),
                    session_of,
                ) {
                    Liveness::Alive => Err(LaneError::Unended { lane, pid }),
                    Liveness::Gone | Liveness::Unseen => Err(LaneError::Unseen { lane, pid }),
                }
            }
            None => Ok(()),
        }
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

    /// Waits while the holder the record names still runs, which a lock taken over a removed lock file cannot tell from one that died, and for as long as its work shows something new.
    #[cfg(unix)]
    fn outwait_holder(
        &self,
        request: &Request<'_>,
        progress: &mut dyn Write,
        (observed, (pid, born)): (&Observation, (u32, &str)),
    ) -> Result<(), LaneError> {
        let lane = request.lane.name();
        let mut reported: Option<Instant> = None;
        let mut watch = Watch::new(self.quiet);
        loop {
            match holder_state(&start_of(pid), born) {
                HolderState::Dead => return Ok(()),
                HolderState::Unseen => return Err(LaneError::HolderUnseen { lane, pid }),
                HolderState::Alive => {}
            }
            if let Some(signal) = request.stops.raised() {
                return Err(LaneError::Interrupted { lane, signal });
            }
            if let Some(silent) = watch.stalled(self.record) {
                return Err(self.stalled(request, silent));
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
            let deadline = watch
                .deadline(reported.unwrap_or(watch.moved))
                .map_err(|source| io(self.record, source))?;
            self.wait(observed, request, deadline)?;
        }
    }

    /// The refusal a run waiting behind this lane's holder gives once the holder has shown nothing new for `silent`.
    fn stalled(&self, request: &Request<'_>, silent: Duration) -> LaneError {
        LaneError::Stalled {
            lane: request.lane.name(),
            holder: describe(self.record),
            silent: span(silent.as_secs()),
            quiet: span(self.quiet.as_secs()),
        }
    }
}

/// What a run waiting behind a holder has seen of the holder's work, and since when it has seen nothing new (ADR 0026).
#[cfg(unix)]
#[derive(Debug)]
struct Watch {
    quiet: Duration,
    look: Duration,
    seen: Option<Vec<u8>>,
    moved: Instant,
    looked: Option<Instant>,
}

#[cfg(unix)]
impl Watch {
    /// A watch that calls a holder stalled once its work has shown nothing new for `quiet`, looking four times within it.
    fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            look: quiet.checked_div(4).unwrap_or(POLL).max(POLL),
            seen: None,
            moved: Instant::now(),
            looked: None,
        }
    }

    /// How long the holder whose record is at `record` has shown nothing new of its work, once that is as long as the window; a look that fails shows nothing.
    fn stalled(&mut self, record: &Path) -> Option<Duration> {
        if self.looked.is_none_or(|last| last.elapsed() >= self.look) {
            self.looked = Some(Instant::now());
            if let Some(shown) = shown(record)
                && self.seen.as_ref() != Some(&shown)
            {
                self.seen = Some(shown);
                self.moved = Instant::now();
            }
        }
        let silent = self.moved.elapsed();
        (silent >= self.quiet).then_some(silent)
    }

    fn deadline(&self, reported: Instant) -> std::io::Result<Instant> {
        let sampled = match self.looked {
            Some(last) => last,
            None => self.moved,
        };
        let deadlines = [
            sampled.checked_add(self.look),
            self.moved.checked_add(self.quiet),
            reported.checked_add(REPORT),
        ];
        let mut next = Instant::now()
            .checked_add(REPORT)
            .ok_or_else(|| std::io::Error::other("the lane reporting deadline is too large"))?;
        for deadline in deadlines {
            next = next.min(deadline.ok_or_else(|| {
                std::io::Error::other("a lane CPU, quiet or reporting deadline is too large")
            })?);
        }
        Ok(next)
    }
}

/// A watch where a group's processes cannot be listed, which is where a stalled holder cannot be told from a slow one, so the lock alone decides.
#[cfg(not(unix))]
#[derive(Debug)]
struct Watch;

#[cfg(not(unix))]
impl Watch {
    /// A watch that never calls a holder stalled, whatever `quiet` says.
    const fn new(_quiet: Duration) -> Self {
        Self
    }

    /// Nothing: a holder is never called stalled where its work cannot be seen.
    #[expect(
        clippy::unused_self,
        clippy::needless_pass_by_ref_mut,
        reason = "the same signature as the platform that can see a holder's work, whose watch remembers what it saw"
    )]
    const fn stalled(&mut self, _record: &Path) -> Option<Duration> {
        None
    }

    #[expect(
        clippy::unused_self,
        reason = "the non-Unix watch only owns the reporting deadline"
    )]
    fn deadline(&self, reported: Instant) -> std::io::Result<Instant> {
        reported
            .checked_add(REPORT)
            .ok_or_else(|| std::io::Error::other("the lane reporting deadline is too large"))
    }
}

/// What the holder whose record is at `record` shows of its work: the record, and every process of each group the record names with the processor time it has used, as `ps` lists them; nothing where either cannot be read.
#[cfg(unix)]
fn shown(record: &Path) -> Option<Vec<u8>> {
    let mut shown = match std::fs::read(record) {
        Ok(bytes) => bytes,
        Err(_unread) => return None,
    };
    let groups: Vec<u32> = Record::read(&shown)
        .groups()
        .iter()
        .map(|group| group.pid)
        .collect();
    let listing = answer(
        Command::new("ps")
            .args(["-A", "-o", "pid=,pgid=,time="])
            .env("LC_ALL", "C"),
    )?;
    let mut rows: Vec<&str> = listing
        .lines()
        .filter(|row| {
            row.split_whitespace()
                .nth(1)
                .and_then(number)
                .is_some_and(|group| groups.contains(&group))
        })
        .collect();
    rows.sort_unstable();
    for row in rows {
        shown.push(b'\n');
        shown.extend_from_slice(row.as_bytes());
    }
    Some(shown)
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

#[cfg(unix)]
fn waiter_state(me: u32, pid: u32, born: Option<&str>, now: &Start) -> HolderState {
    if pid == me {
        return HolderState::Alive;
    }
    match now {
        Start::Running(started) if born.is_none_or(|born| born.is_empty() || born == started) => {
            HolderState::Alive
        }
        Start::Running(_) | Start::Absent => HolderState::Dead,
        Start::Unread => HolderState::Unseen,
    }
}

#[cfg(all(test, unix))]
mod waiter_tests {
    use super::{HolderState, Start, waiter_state};

    #[test]
    fn a_waiting_run_is_removed_only_when_absent_or_recycled() {
        let cases = [
            (7, Some("before"), Start::Unread, HolderState::Alive),
            (
                8,
                Some("before"),
                Start::Running("before".to_owned()),
                HolderState::Alive,
            ),
            (
                8,
                None,
                Start::Running("now".to_owned()),
                HolderState::Alive,
            ),
            (
                8,
                Some(""),
                Start::Running("now".to_owned()),
                HolderState::Alive,
            ),
            (
                8,
                Some("before"),
                Start::Running("now".to_owned()),
                HolderState::Dead,
            ),
            (8, Some("before"), Start::Absent, HolderState::Dead),
            (8, Some("before"), Start::Unread, HolderState::Unseen),
        ];
        for (pid, born, now, expected) in cases {
            assert_eq!(waiter_state(7, pid, born, &now), expected);
        }
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

/// The byte the next writer ends a line with that a writer was killed before finishing, which no finished line holds.
const TORN: u8 = 0;

/// A lane's record as its writers finished it: every line that ends in a newline, holds no torn mark and is text, and every piece that is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<'a> {
    lines: Vec<&'a str>,
    unread: Vec<&'a [u8]>,
}

impl<'a> Record<'a> {
    /// `bytes` as the writers of a lane's record finished them.
    #[must_use]
    pub fn read(bytes: &'a [u8]) -> Self {
        let mut record = Self {
            lines: Vec::new(),
            unread: Vec::new(),
        };
        for piece in bytes.split_inclusive(|byte| *byte == b'\n') {
            match piece.strip_suffix(b"\n") {
                Some(line) if !line.contains(&TORN) => match std::str::from_utf8(line) {
                    Ok(text) => record.lines.push(text),
                    Err(_not_text) => record.unread.push(line),
                },
                Some(torn) => {
                    let mut unfinished = torn;
                    while let Some(before) = unfinished.strip_suffix(&[TORN]) {
                        unfinished = before;
                    }
                    if !unfinished.is_empty() {
                        record.unread.push(unfinished);
                    }
                }
                None => record.unread.push(piece),
            }
        }
        record
    }

    /// Every piece that is no finished line: one a writer was killed before finishing, one still being written, or one that is not text, each as a person can read it.
    #[cfg(unix)]
    #[must_use]
    pub fn unread(&self) -> Vec<String> {
        self.unread
            .iter()
            .map(|piece| piece.escape_ascii().to_string())
            .collect()
    }

    /// The value the first finished line that says `name=` gives it.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&'a str> {
        self.lines
            .iter()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
    }

    /// Every group a finished line names under the holder the record says wrote it, whether or not that holder has let the lane go.
    #[cfg(unix)]
    fn groups(&self) -> Vec<Recorded> {
        let holder = self.field("pid");
        self.lines
            .iter()
            .filter_map(|line| {
                line.strip_prefix("group=")
                    .or_else(|| line.strip_prefix("leader="))
            })
            .filter_map(|group| recorded(group, holder))
            .collect()
    }

    /// Every group a finished line names under the holder that wrote it, or none when that holder let the lane go itself or wrote the record in a boot other than `this_boot`.
    #[cfg(unix)]
    #[must_use]
    pub fn unreleased(&self, this_boot: Option<&str>) -> Vec<Recorded> {
        let written_in = self.field("boot").filter(|then| !then.is_empty());
        if let (Some(then), Some(now)) = (written_in, this_boot)
            && then != now
        {
            return Vec::new();
        }
        if self.lines.contains(&"released") {
            return Vec::new();
        }
        self.groups()
    }
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
            match append(record, "released") {
                Ok(()) | Err(_) => {}
            }
            let held = self.lock.take();
            drop(held);
            let publication = record.with_extension("released");
            if let Err(source) = replace(&publication, &std::process::id().to_string()) {
                drop(source);
                std::process::abort();
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
        let holder = match std::fs::read(record) {
            Ok(bytes) => Record::read(&bytes).field("pid").unwrap_or("").to_owned(),
            Err(_no_record_yet) => String::new(),
        };
        let session = session_text(leader);
        append(
            record,
            &format!("group={leader} holder={holder} session={session} born={born}"),
        )
    }
}

/// The branch and short commit of the checkout at `directory`, or a dash for each that cannot be read; no `GIT_*` variable of `environment` reaches the git it asks.
#[must_use]
pub fn revision_of(directory: &Path, environment: &Environment) -> String {
    let ask = |arguments: &[&str]| {
        let mut git = crate::repository::git(directory);
        for name in environment.beginning("GIT_") {
            git.env_remove(name);
        }
        answer(git.args(arguments)).unwrap_or_else(|| "-".to_owned())
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

fn state_directory(environment: &Environment) -> Option<PathBuf> {
    if let Some(state) = environment.value("XDG_STATE_HOME") {
        return Some(PathBuf::from(state));
    }
    if let Some(home) = environment.value("HOME") {
        return Some(Path::new(home).join(".local").join("state"));
    }
    environment.value("LOCALAPPDATA").map(PathBuf::from)
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
    let bytes = match std::fs::read(record) {
        Ok(bytes) => bytes,
        Err(_unwritten) => return "a run that has not written its record yet".to_owned(),
    };
    let written = Record::read(&bytes);
    let field = |name: &str| written.field(name).unwrap_or("?").to_owned();
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

/// Appends `line` to the record at `path` in one write under the record's lock, first ending with the torn mark any line a writer was killed before finishing, so the two are never read as one.
fn append(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let mut record = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    record.lock()?;
    let unfinished = if record.metadata()?.len() == 0 {
        false
    } else {
        record.seek(SeekFrom::End(-1))?;
        let mut last = [0_u8; 1];
        record.read_exact(&mut last)?;
        last != *b"\n"
    };
    let mut entry = Vec::with_capacity(line.len().saturating_add(3));
    if unfinished {
        entry.extend_from_slice(&[TORN, b'\n']);
    }
    entry.extend_from_slice(line.as_bytes());
    entry.push(b'\n');
    record.write_all(&entry)
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
