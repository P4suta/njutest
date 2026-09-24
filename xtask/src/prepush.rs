// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The pre-push gate: the exact commit being pushed, checked in the repository's one reusable tree, one whole-workspace run at a time.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::lanes::{self, Holder, Lane, LaneError, Lanes};

/// The object id Git gives a ref that is being deleted, or one the remote does not have yet.
const ZERO: &str = "0000000000000000000000000000000000000000";

/// How often the gate looks at the check it is waiting for.
const POLL: Duration = Duration::from_millis(200);

/// How long a check that was asked to stop has before it is killed.
const GRACE: Duration = Duration::from_secs(5);

/// How long a pass answers for a second push of the same commit against the same base.
const REMEMBERED: Duration = Duration::from_secs(3600);

/// What the gate is handed by the process that runs it.
#[derive(Debug, Clone, Copy)]
pub struct Surroundings<'a> {
    /// The checkout the push was started in.
    pub directory: &'a Path,
    /// The environment the gate was started with; no `GIT_*` variable in it reaches anything the gate starts.
    pub environment: &'a [(OsString, OsString)],
    /// The program running the gate, whose bytes are part of what a remembered pass is about.
    pub executable: &'a Path,
}

/// How a push the gate let through was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passed {
    /// The check ran on this commit and passed.
    Checked,
    /// This commit against this base passed this same gate within the hour.
    Remembered,
}

/// Why the gate refused a push, or could not answer for it.
#[derive(Debug, Error)]
pub enum PrePushError {
    /// A line Git handed the hook did not name two refs and two objects.
    #[error("git supplied an incomplete ref update: {line:?}")]
    Incomplete {
        /// The line as it arrived.
        line: String,
    },
    /// The push names an object other than the checked-out commit.
    #[error(
        "{local_ref} names {local}, but the checked-out commit is {head}\ncheck out the exact commit being pushed before running its gate"
    )]
    NotHead {
        /// The local ref being pushed.
        local_ref: String,
        /// The object it names.
        local: String,
        /// The checked-out commit.
        head: String,
    },
    /// The remote's commit is not here, so nothing can say whether the push is a fast-forward.
    #[error(
        "remote {remote_ref} names commit {remote}, which is not present locally\nfetch the remote ref before proving that this update is a fast-forward"
    )]
    UnknownRemote {
        /// The remote ref.
        remote_ref: String,
        /// The object it names.
        remote: String,
    },
    /// The push would move the remote ref somewhere that does not descend from where it is.
    #[error(
        "{local} does not descend from {remote_ref} at {remote}\nnon-fast-forward updates are forbidden"
    )]
    NotFastForward {
        /// The object being pushed.
        local: String,
        /// The remote ref.
        remote_ref: String,
        /// Where it is now.
        remote: String,
    },
    /// Every update was a deletion, which leaves no object to check.
    #[error("no non-delete ref update was supplied; refusing an unverifiable gate")]
    NothingToCheck,
    /// The gate's tree stopped being the pushed commit while the check ran.
    #[error("HEAD moved from {head} to {now} while its gate ran")]
    Moved {
        /// The commit being pushed.
        head: String,
        /// Where the tree's HEAD is now.
        now: String,
    },
    /// The check left the tree different from the commit it was checking.
    #[error("the isolated check changed the tree of {head}")]
    Changed {
        /// The commit being pushed.
        head: String,
    },
    /// The check outlived its budget and was stopped.
    #[error(
        "the gate passed its {budget}s budget and was stopped at {elapsed}s\nthat is a report about the gate. Find what stopped being cached, or raise NJUTEST_PUSH_BUDGET_SECONDS in a commit that says why"
    )]
    Budget {
        /// The budget, in seconds.
        budget: u64,
        /// When it was stopped, in seconds.
        elapsed: u64,
    },
    /// The check ran and failed; its own output says why.
    #[error("the check failed: {status}")]
    Failed {
        /// How it ended.
        status: String,
    },
    /// A program the gate needs could not be started.
    #[error("{program} could not be started: {source}")]
    Start {
        /// The program.
        program: String,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// Git refused something the gate needed to know or do.
    #[error("git {arguments} failed ({status}): {stderr}")]
    Git {
        /// The arguments Git was given.
        arguments: String,
        /// How it ended.
        status: String,
        /// What it said.
        stderr: String,
    },
    /// A file or directory of the gate's own could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// A setting in the environment was not a number of seconds.
    #[error("{name} is not a number of seconds: {value:?}")]
    Setting {
        /// The variable.
        name: &'static str,
        /// What it held.
        value: String,
    },
    /// Something the gate reads as text was not UTF-8.
    #[error("{what} is not UTF-8 text")]
    NotText {
        /// What was read.
        what: String,
    },
    /// Nothing in the environment says where the gate may keep its tree.
    #[error("no directory for the gate's tree: set NJUTEST_PRE_PUSH_CACHE, XDG_CACHE_HOME or HOME")]
    Nowhere,
    /// This machine's lane for whole-workspace runs could not be held.
    #[error(transparent)]
    Lane {
        /// Why.
        #[from]
        source: LaneError,
    },
    /// Whom the gate is waiting for, or what it found, could not be said.
    #[error("the gate's progress could not be written: {source}")]
    Progress {
        /// The output failure.
        source: std::io::Error,
    },
}

impl PrePushError {
    /// The exit status the hook reports this refusal with.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Budget { .. } => 124,
            Self::Incomplete { .. }
            | Self::NotHead { .. }
            | Self::UnknownRemote { .. }
            | Self::NotFastForward { .. }
            | Self::NothingToCheck
            | Self::Moved { .. }
            | Self::Changed { .. }
            | Self::Failed { .. }
            | Self::Start { .. }
            | Self::Git { .. }
            | Self::Io { .. }
            | Self::Setting { .. }
            | Self::NotText { .. }
            | Self::Nowhere
            | Self::Lane { .. }
            | Self::Progress { .. } => 1,
        }
    }
}

/// Checks the commit being pushed and says whether it may go.
///
/// # Errors
/// Returns a [`PrePushError`] naming what was refused, or what kept the gate from answering.
pub fn gate(
    surroundings: &Surroundings<'_>,
    updates: &mut dyn BufRead,
    progress: &mut dyn Write,
) -> Result<Passed, PrePushError> {
    let tools = Tools {
        environment: surroundings.environment,
    };
    let here = surroundings.directory;
    let head = tools.answer(here, &["rev-parse", "--verify", "HEAD"])?;
    verify(&tools, here, &head, &read_updates(updates)?)?;
    let settings = Settings::from_environment(surroundings.environment)?;
    let place = Place::of(&tools, here, &settings.cache)?;
    let base = tools.maybe(
        here,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{}^{{commit}}", settings.base_ref),
        ],
    )?;
    let memory = place.memory(&head, base.as_deref(), &identity(surroundings.executable)?);
    if remembered(&memory)? {
        say(
            progress,
            &format!(
                "pre-push: {head} against {} already passed this gate within the hour; it is not run again",
                base.as_deref().unwrap_or("no base")
            ),
        )?;
        return Ok(Passed::Remembered);
    }
    let lanes = Lanes::from_environment(surroundings.environment)?;
    let holder = Holder {
        worktree: here.to_path_buf(),
        revision: lanes::revision_of(here),
        command: format!("the pre-push gate for {head}"),
    };
    let turn = lanes.hold(Lane::Heavy, &holder, progress)?;
    place.prepare(&tools, here, &head)?;
    let run = Run {
        tools,
        place: &place,
        head: &head,
        settings: &settings,
        lanes: &lanes,
    };
    let checked = check(&run, progress);
    if let Err(failure) = tools.restore(&place.tree) {
        say(
            progress,
            &format!(
                "pre-push: the gate's tree could not be put back ({failure}); the next push makes it again"
            ),
        )?;
    }
    checked?;
    remember(&memory, &head)?;
    drop(turn);
    Ok(Passed::Checked)
}

/// Everything one check of one pushed commit is about.
#[derive(Debug, Clone, Copy)]
struct Run<'a> {
    tools: Tools<'a>,
    place: &'a Place,
    head: &'a str,
    settings: &'a Settings,
    lanes: &'a Lanes,
}

fn check(run: &Run<'_>, progress: &mut dyn Write) -> Result<(), PrePushError> {
    run.place.require_exact(&run.tools, run.head)?;
    let warming = Instant::now();
    let warmed = run
        .place
        .check_command(&run.tools, run.head, run.lanes)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let spent = warming.elapsed().as_secs();
    let line = match warmed {
        Ok(_decides_nothing) => {
            format!("pre-push: compiled in {spent}s, which the budget does not count")
        }
        Err(source) => {
            format!("pre-push: the warming pass could not start after {spent}s: {source}")
        }
    };
    say(progress, &line)?;
    within_budget(
        run.place.check_command(&run.tools, run.head, run.lanes),
        run.settings,
        progress,
    )?;
    run.place.require_exact(&run.tools, run.head)
}

fn within_budget(
    mut command: Command,
    settings: &Settings,
    progress: &mut dyn Write,
) -> Result<(), PrePushError> {
    grouped(&mut command);
    let started = Instant::now();
    let mut check = Check::launch(command.stdin(Stdio::null()))?;
    let status = loop {
        if let Some(status) = check.try_wait()? {
            break status;
        }
        if started.elapsed() >= settings.budget {
            check.stop()?;
            return Err(PrePushError::Budget {
                budget: settings.budget.as_secs(),
                elapsed: started.elapsed().as_secs(),
            });
        }
        std::thread::sleep(POLL);
    };
    if !status.success() {
        return Err(PrePushError::Failed {
            status: status.to_string(),
        });
    }
    let elapsed = started.elapsed();
    if elapsed >= settings.expected {
        say(
            progress,
            &format!(
                "pre-push: the gate passed in {}s, over the {}s a warm run should beat\npre-push: that is the reading to act on while it is still cheap. A first run after a merge is expected here; a second one that is still slow means something stopped being cached",
                elapsed.as_secs(),
                settings.expected.as_secs()
            ),
        )?;
    }
    Ok(())
}

/// The budgeted check, owned so that it and everything in its process group are reaped on every path.
#[derive(Debug)]
struct Check {
    child: Option<Child>,
}

impl Check {
    fn launch(command: &mut Command) -> Result<Self, PrePushError> {
        command
            .spawn()
            .map(|child| Self { child: Some(child) })
            .map_err(|source| PrePushError::Start {
                program: "mise run check".to_owned(),
                source,
            })
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, PrePushError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let status = child
            .try_wait()
            .map_err(|source| io_error("the check", source))?;
        if status.is_some() {
            self.child = None;
        }
        Ok(status)
    }

    fn stop(&mut self) -> Result<(), PrePushError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        signal_group(child, Stop::Ask)?;
        let asked = Instant::now();
        while asked.elapsed() < GRACE {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        signal_group(child, Stop::Kill)?;
        child
            .wait()
            .map_err(|source| io_error("the check", source))?;
        self.child = None;
        Ok(())
    }
}

impl Drop for Check {
    fn drop(&mut self) {
        if self.stop().is_err() {
            std::process::abort();
        }
    }
}

/// How hard a check that outlived its budget is stopped.
#[derive(Debug, Clone, Copy)]
enum Stop {
    /// `SIGTERM` to its whole process group.
    Ask,
    /// `SIGKILL` to its whole process group.
    Kill,
}

#[cfg(unix)]
fn grouped(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
const fn grouped(_command: &mut Command) {}

#[cfg(unix)]
fn signal_group(child: &mut Child, how: Stop) -> Result<(), PrePushError> {
    let signal = match how {
        Stop::Ask => rustix::process::Signal::TERM,
        Stop::Kill => rustix::process::Signal::KILL,
    };
    let group = match i32::try_from(child.id()) {
        Ok(raw) => rustix::process::Pid::from_raw(raw),
        Err(_beyond_a_pid) => None,
    };
    let Some(group) = group else {
        return signal_alone(child, how);
    };
    match rustix::process::kill_process_group(group, signal) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(errno) => Err(io_error(
            "the check's process group",
            std::io::Error::from(errno),
        )),
    }
}

#[cfg(not(unix))]
fn signal_group(child: &mut Child, how: Stop) -> Result<(), PrePushError> {
    signal_alone(child, how)
}

fn signal_alone(child: &mut Child, how: Stop) -> Result<(), PrePushError> {
    match how {
        Stop::Ask => Ok(()),
        Stop::Kill => child.kill().map_err(|source| io_error("the check", source)),
    }
}

/// One ref update as Git hands it to the hook.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Update {
    local_ref: String,
    local: String,
    remote_ref: String,
    remote: String,
}

fn read_updates(input: &mut dyn BufRead) -> Result<Vec<Update>, PrePushError> {
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|source| io_error("the ref updates on standard input", source))?;
    text.lines()
        .map(|line| {
            let mut fields = line.split_whitespace().map(str::to_owned);
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some(local_ref), Some(local), Some(remote_ref), Some(remote)) => Ok(Update {
                    local_ref,
                    local,
                    remote_ref,
                    remote,
                }),
                _incomplete => Err(PrePushError::Incomplete {
                    line: line.to_owned(),
                }),
            }
        })
        .collect()
}

fn verify(
    tools: &Tools<'_>,
    here: &Path,
    head: &str,
    updates: &[Update],
) -> Result<(), PrePushError> {
    let pushed: Vec<&Update> = updates
        .iter()
        .filter(|update| update.local != ZERO)
        .collect();
    for update in &pushed {
        if update.local != head {
            return Err(PrePushError::NotHead {
                local_ref: update.local_ref.clone(),
                local: update.local.clone(),
                head: head.to_owned(),
            });
        }
        if update.remote == ZERO {
            continue;
        }
        let known = format!("{}^{{commit}}", update.remote);
        if !tools.succeeds(here, &["cat-file", "-e", &known])? {
            return Err(PrePushError::UnknownRemote {
                remote_ref: update.remote_ref.clone(),
                remote: update.remote.clone(),
            });
        }
        if !tools.succeeds(
            here,
            &["merge-base", "--is-ancestor", &update.remote, &update.local],
        )? {
            return Err(PrePushError::NotFastForward {
                local: update.local.clone(),
                remote_ref: update.remote_ref.clone(),
                remote: update.remote.clone(),
            });
        }
    }
    if pushed.is_empty() {
        return Err(PrePushError::NothingToCheck);
    }
    Ok(())
}

/// What the gate reads from its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    budget: Duration,
    expected: Duration,
    base_ref: String,
    cache: PathBuf,
}

impl Settings {
    fn from_environment(environment: &[(OsString, OsString)]) -> Result<Self, PrePushError> {
        Ok(Self {
            budget: seconds(environment, "NJUTEST_PUSH_BUDGET_SECONDS", 600)?,
            expected: seconds(environment, "NJUTEST_PUSH_EXPECTED_SECONDS", 420)?,
            base_ref: match lanes::variable(environment, "NJUTEST_COMMITTED_BASE_REF") {
                Some(named) => text_of("NJUTEST_COMMITTED_BASE_REF", named)?.to_owned(),
                None => "origin/main".to_owned(),
            },
            cache: cache_root(environment).ok_or(PrePushError::Nowhere)?,
        })
    }
}

fn seconds(
    environment: &[(OsString, OsString)],
    name: &'static str,
    default: u64,
) -> Result<Duration, PrePushError> {
    let Some(value) = lanes::variable(environment, name) else {
        return Ok(Duration::from_secs(default));
    };
    let text = text_of(name, value)?;
    match text.trim().parse::<u64>() {
        Ok(count) => Ok(Duration::from_secs(count)),
        Err(_not_a_count) => Err(PrePushError::Setting {
            name,
            value: text.to_owned(),
        }),
    }
}

fn cache_root(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    if let Some(named) = lanes::variable(environment, "NJUTEST_PRE_PUSH_CACHE") {
        return Some(PathBuf::from(named));
    }
    let caches = platform_caches(environment)?;
    Some(caches.join("njutest").join("pre-push"))
}

#[cfg(target_os = "macos")]
fn platform_caches(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    lanes::variable(environment, "HOME").map(|home| Path::new(home).join("Library").join("Caches"))
}

#[cfg(not(target_os = "macos"))]
fn platform_caches(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    if let Some(caches) = lanes::variable(environment, "XDG_CACHE_HOME") {
        return Some(PathBuf::from(caches));
    }
    if let Some(home) = lanes::variable(environment, "HOME") {
        return Some(Path::new(home).join(".cache"));
    }
    lanes::variable(environment, "LOCALAPPDATA").map(PathBuf::from)
}

/// Where one repository's gate keeps its tree, its build, and what it has passed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    home: PathBuf,
    tree: PathBuf,
    target: PathBuf,
}

impl Place {
    fn of(tools: &Tools<'_>, here: &Path, cache: &Path) -> Result<Self, PrePushError> {
        let common = tools.answer(
            here,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = std::fs::canonicalize(&common).map_err(|source| io_error(&common, source))?;
        let home = cache.join(short(common.as_os_str().as_encoded_bytes()));
        Ok(Self {
            tree: home.join("tree"),
            target: home.join("target"),
            home,
        })
    }

    fn memory(&self, head: &str, base: Option<&str>, identity: &str) -> PathBuf {
        let mut digest = Sha256::new();
        for part in [head, base.unwrap_or("no base"), identity] {
            digest.update(part.as_bytes());
            digest.update(b"\n");
        }
        self.home
            .join("passed")
            .join(hex::encode(digest.finalize()))
    }

    fn prepare(&self, tools: &Tools<'_>, here: &Path, head: &str) -> Result<(), PrePushError> {
        std::fs::create_dir_all(&self.home).map_err(|source| io_error(&self.home, source))?;
        tools.run(here, &[OsStr::new("worktree"), OsStr::new("prune")])?;
        if !self.moved_to(tools, head)? {
            match std::fs::remove_dir_all(&self.tree) {
                Ok(()) => {}
                Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_error(&self.tree, source)),
            }
            tools.run(here, &[OsStr::new("worktree"), OsStr::new("prune")])?;
            tools.run(
                here,
                &[
                    OsStr::new("worktree"),
                    OsStr::new("add"),
                    OsStr::new("--quiet"),
                    OsStr::new("--detach"),
                    self.tree.as_os_str(),
                    OsStr::new(head),
                ],
            )?;
        }
        self.link_target()
    }

    fn moved_to(&self, tools: &Tools<'_>, head: &str) -> Result<bool, PrePushError> {
        let marker = self.tree.join(".git");
        let present = marker
            .try_exists()
            .map_err(|source| io_error(&marker, source))?;
        if !present {
            return Ok(false);
        }
        let clean = tools
            .maybe(
                &self.tree,
                &["status", "--porcelain=v1", "--untracked-files=all"],
            )?
            .is_some_and(|status| status.is_empty());
        if !clean {
            return Ok(false);
        }
        tools.succeeds(&self.tree, &["checkout", "--quiet", "--detach", head])
    }

    #[cfg(unix)]
    fn link_target(&self) -> Result<(), PrePushError> {
        let inside = self.tree.join("target");
        std::fs::create_dir_all(&inside).map_err(|source| io_error(&inside, source))?;
        for profile in ["debug", "release"] {
            let kept = self.target.join(profile);
            std::fs::create_dir_all(&kept).map_err(|source| io_error(&kept, source))?;
            let link = inside.join(profile);
            match std::fs::remove_file(&link) {
                Ok(()) => {}
                Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_error(&link, source)),
            }
            std::os::unix::fs::symlink(&kept, &link).map_err(|source| io_error(&link, source))?;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn link_target(&self) -> Result<(), PrePushError> {
        std::fs::create_dir_all(&self.target).map_err(|source| io_error(&self.target, source))
    }

    fn check_command(&self, tools: &Tools<'_>, head: &str, lanes: &Lanes) -> Command {
        let mut command = tools.command("mise");
        command
            .args(["run", "check"])
            .current_dir(&self.tree)
            .env("NJUTEST_COMMITTED_HEAD", head)
            .env(lanes::HELD, lanes.held_with(Lane::Heavy));
        if cfg!(not(unix)) {
            command.env("CARGO_TARGET_DIR", &self.target);
        }
        command
    }

    fn require_exact(&self, tools: &Tools<'_>, head: &str) -> Result<(), PrePushError> {
        let now = tools.answer(&self.tree, &["rev-parse", "--verify", "HEAD"])?;
        if now != head {
            return Err(PrePushError::Moved {
                head: head.to_owned(),
                now,
            });
        }
        let status = tools.answer(
            &self.tree,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        if !status.is_empty() {
            return Err(PrePushError::Changed {
                head: head.to_owned(),
            });
        }
        Ok(())
    }
}

/// Git and the check, started without any `GIT_*` variable a hook was handed, so each answers about the directory it is started in.
#[derive(Debug, Clone, Copy)]
struct Tools<'a> {
    environment: &'a [(OsString, OsString)],
}

impl Tools<'_> {
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        for (name, _value) in self.environment {
            if name.as_encoded_bytes().starts_with(b"GIT_") {
                command.env_remove(name);
            }
        }
        command
    }

    fn output(
        &self,
        here: &Path,
        arguments: &[&OsStr],
    ) -> Result<std::process::Output, PrePushError> {
        self.command("git")
            .args(arguments)
            .current_dir(here)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| PrePushError::Start {
                program: "git".to_owned(),
                source,
            })
    }

    fn run(&self, here: &Path, arguments: &[&OsStr]) -> Result<String, PrePushError> {
        let output = self.output(here, arguments)?;
        let rendered = || {
            arguments
                .iter()
                .map(|argument| argument.display().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        };
        if output.status.success() {
            return match String::from_utf8(output.stdout) {
                Ok(text) => Ok(text.trim().to_owned()),
                Err(_not_text) => Err(PrePushError::NotText {
                    what: format!("what git {} printed", rendered()),
                }),
            };
        }
        Err(PrePushError::Git {
            arguments: rendered(),
            status: output.status.to_string(),
            stderr: escaped(output.stderr).trim().to_owned(),
        })
    }

    fn answer(&self, here: &Path, arguments: &[&str]) -> Result<String, PrePushError> {
        self.run(here, &spelled(arguments))
    }

    fn maybe(&self, here: &Path, arguments: &[&str]) -> Result<Option<String>, PrePushError> {
        if !self.succeeds(here, arguments)? {
            return Ok(None);
        }
        self.answer(here, arguments).map(Some)
    }

    fn succeeds(&self, here: &Path, arguments: &[&str]) -> Result<bool, PrePushError> {
        Ok(self.output(here, &spelled(arguments))?.status.success())
    }

    fn restore(&self, tree: &Path) -> Result<ExitStatus, PrePushError> {
        self.command("git")
            .args(["restore", "--staged", "--worktree", ":/"])
            .current_dir(tree)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|source| PrePushError::Start {
                program: "git restore".to_owned(),
                source,
            })
    }
}

fn spelled<'a>(arguments: &[&'a str]) -> Vec<&'a OsStr> {
    arguments
        .iter()
        .map(|argument| OsStr::new(*argument))
        .collect()
}

fn identity(executable: &Path) -> Result<String, PrePushError> {
    let bytes = std::fs::read(executable).map_err(|source| io_error(executable, source))?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

fn short(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
        .chars()
        .take(12)
        .collect()
}

fn remembered(memory: &Path) -> Result<bool, PrePushError> {
    let written = match std::fs::metadata(memory) {
        Ok(metadata) => metadata
            .modified()
            .map_err(|source| io_error(memory, source))?,
        Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(io_error(memory, source)),
    };
    Ok(written.elapsed().is_ok_and(|age| age < REMEMBERED))
}

fn remember(memory: &Path, head: &str) -> Result<(), PrePushError> {
    if let Some(passed) = memory.parent() {
        std::fs::create_dir_all(passed).map_err(|source| io_error(passed, source))?;
    }
    std::fs::write(memory, format!("{head}\n")).map_err(|source| io_error(memory, source))
}

fn text_of<'a>(name: &str, value: &'a OsStr) -> Result<&'a str, PrePushError> {
    value.to_str().ok_or_else(|| PrePushError::NotText {
        what: format!("the value of {name}"),
    })
}

fn escaped(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(invalid) => invalid.into_bytes().escape_ascii().to_string(),
    }
}

fn say(progress: &mut dyn Write, line: &str) -> Result<(), PrePushError> {
    writeln!(progress, "{line}").map_err(|source| PrePushError::Progress { source })
}

fn io_error(path: impl AsRef<Path>, source: std::io::Error) -> PrePushError {
    PrePushError::Io {
        path: path.as_ref().display().to_string(),
        source,
    }
}
