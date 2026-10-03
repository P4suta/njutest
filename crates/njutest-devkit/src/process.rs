// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One command's answer, in the shape a spawned one has.

use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output};

use crate::thread::{JoinError, ScopedThread};
use njutest_process::GroupChild;

mod ready;

pub use ready::{ReadyPath, ReadyState};

/// Interprets a test protocol's bytes as UTF-8 without replacing invalid input.
///
/// # Panics
/// The test subject emitted bytes outside the UTF-8 protocol the test asserts.
#[must_use]
#[track_caller]
#[expect(
    clippy::panic,
    reason = "non-UTF-8 process output is a protocol failure in a test oracle"
)]
pub fn strict_utf8(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => std::borrow::Cow::Borrowed(text),
        Err(error) => panic!("test protocol output is not UTF-8: {error}"),
    }
}

/// A child-process ownership transition that could not be completed.
#[derive(Debug, thiserror::Error)]
pub enum ChildError {
    /// The command could not be started.
    #[error("the supervised child could not be started: {source}")]
    Start {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// The child had already been consumed by an earlier terminal operation.
    #[error("the supervised child had already been reaped")]
    AlreadyReaped,
    /// The operating system could not observe or reap the child.
    #[error("the supervised child could not be observed or reaped: {source}")]
    Reap {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// Waiting or collecting one or both output pipes failed after ownership had been closed by reaping the child.
    #[error(
        "the supervised child's output could not be collected (wait={wait:?}, stdout={stdout:?}, stderr={stderr:?})"
    )]
    Output {
        /// The failure to wait for the child, when waiting failed.
        wait: Option<std::io::Error>,
        /// The failure to read or join the stdout collector, when it failed.
        stdout: Option<ProcessOutputError>,
        /// The failure to read or join the stderr collector, when it failed.
        stderr: Option<ProcessOutputError>,
    },
}

/// Why one owned output-pipe collector could not return all of its bytes.
#[derive(Debug, thiserror::Error)]
#[error("{kind}: {source}")]
pub struct ProcessOutputError {
    /// Which closed collector transition failed.
    kind: OutputFailureKind,
    /// The underlying read or typed-join failure.
    #[source]
    source: std::io::Error,
}

impl ProcessOutputError {
    const fn read(source: std::io::Error) -> Self {
        Self {
            kind: OutputFailureKind::Read,
            source,
        }
    }

    fn join(source: JoinError) -> Self {
        Self {
            kind: OutputFailureKind::Join,
            source: std::io::Error::other(source),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputFailureKind {
    Read,
    Join,
}

impl std::fmt::Display for OutputFailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => formatter.write_str("the pipe could not be read"),
            Self::Join => formatter.write_str("the pipe collector could not be joined"),
        }
    }
}

/// A child process whose owner reaps it on every exit path.
#[derive(Debug)]
pub struct SupervisedChild {
    owner: ChildOwner,
}

impl SupervisedChild {
    /// Starts `command` and takes ownership of its child.
    ///
    /// # Errors
    /// Returns [`ChildError::Start`] when the operating system refuses the command.
    pub fn launch(command: &mut Command) -> Result<Self, ChildError> {
        ChildOwner::launch(command).map(|owner| Self { owner })
    }

    /// Starts an original native session that retains naturally released nested producers.
    ///
    /// # Errors
    /// Native creation or custody publication refused after mandatory cleanup.
    #[cfg(unix)]
    pub fn launch_session(command: &mut Command) -> Result<Self, ChildError> {
        njutest_process::SessionOwner::launch(command)
            .map(|session| Self {
                owner: ChildOwner {
                    child: ChildScope::Session(session),
                },
            })
            .map_err(|source| ChildError::Start { source })
    }

    /// The operating-system process identifier, while the child is live.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.owner.live().and_then(GroupChild::id)
    }

    /// Retains the non-reaping completion event while this child still owns its producer group.
    ///
    /// # Errors
    /// The terminal child has already been consumed.
    pub fn completion(&self) -> Result<std::sync::Arc<njutest_process::ChildEvent>, ChildError> {
        self.owner
            .live()
            .map(GroupChild::completion)
            .ok_or(ChildError::AlreadyReaped)
    }

    /// Takes the child's standard input pipe, when one was configured.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.owner.live_mut()?.stdin()
    }

    /// Takes the child's standard output pipe, when one was configured.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.owner.live_mut()?.stdout()
    }

    /// Takes the child's standard error pipe, when one was configured.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.owner.live_mut()?.stderr()
    }

    /// Observes whether the child has exited without waiting.
    ///
    /// # Errors
    /// Returns a typed ownership or operating-system failure.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ChildError> {
        self.owner.try_wait()
    }

    /// Waits for and reaps the child.
    ///
    /// # Errors
    /// Returns a typed ownership or operating-system failure.
    pub fn wait(&mut self) -> Result<ExitStatus, ChildError> {
        self.owner.wait()
    }

    /// Observes natural leader exit while retaining its complete scope until owner disposal.
    ///
    /// # Errors
    /// Observation failures trigger complete settlement and remain retained by the native owner.
    #[cfg(unix)]
    pub fn observe_status(&mut self) -> Result<ExitStatus, ChildError> {
        self.owner
            .live_mut()
            .ok_or(ChildError::AlreadyReaped)?
            .observe_status()
            .map_err(|source| ChildError::Reap { source })
    }

    /// Reaps this leader while retaining only one validated original native member.
    ///
    /// # Errors
    /// Native membership or cleanup refused after mandatory original group settlement.
    #[cfg(unix)]
    pub fn reap_to_member(
        &mut self,
        member: njutest_process::ForeignProcess,
    ) -> Result<njutest_process::NamedMemberCompletion, ChildError> {
        let completion = self
            .owner
            .live_mut()
            .ok_or(ChildError::AlreadyReaped)?
            .reap_to_member(member)
            .map_err(|source| ChildError::Reap { source })?;
        self.owner.child.close();
        Ok(completion)
    }

    /// Waits for the child and collects its configured output pipes.
    ///
    /// # Errors
    /// Returns a typed ownership or operating-system failure.
    pub fn wait_with_output(mut self) -> Result<Output, ChildError> {
        self.owner.wait_with_output()
    }
}

/// A producer group whose terminal transitions include every member and owned pipe collector.
#[derive(Debug)]
struct ChildOwner {
    /// A terminal child settles at wait; an original session stays retained until owner disposal.
    child: ChildScope,
}

#[derive(Debug)]
enum ChildScope {
    Terminal(Option<GroupChild>),
    #[cfg(unix)]
    Session(njutest_process::SessionOwner),
}

impl ChildScope {
    const fn as_ref(&self) -> Option<&GroupChild> {
        match self {
            Self::Terminal(child) => child.as_ref(),
            #[cfg(unix)]
            Self::Session(session) => Some(session.scope()),
        }
    }

    const fn as_mut(&mut self) -> Option<&mut GroupChild> {
        match self {
            Self::Terminal(child) => child.as_mut(),
            #[cfg(unix)]
            Self::Session(session) => Some(session.scope_mut()),
        }
    }

    fn close(&mut self) {
        match self {
            Self::Terminal(child) => *child = None,
            #[cfg(unix)]
            Self::Session(_session) => {}
        }
    }
}

impl ChildOwner {
    fn launch(command: &mut Command) -> Result<Self, ChildError> {
        GroupChild::start(command)
            .map(|child| Self {
                child: ChildScope::Terminal(Some(child)),
            })
            .map_err(|source| ChildError::Start { source })
    }

    const fn live(&self) -> Option<&GroupChild> {
        self.child.as_ref()
    }

    const fn live_mut(&mut self) -> Option<&mut GroupChild> {
        self.child.as_mut()
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, ChildError> {
        #[cfg(unix)]
        if let ChildScope::Session(session) = &mut self.child {
            return session
                .try_observe_status()
                .map_err(|source| ChildError::Reap { source });
        }
        let child = self.child.as_mut().ok_or(ChildError::AlreadyReaped)?;
        let status = child
            .try_wait_status()
            .map_err(|source| ChildError::Reap { source })?;
        if status.is_some() {
            self.child.close();
        }
        Ok(status)
    }

    fn wait(&mut self) -> Result<ExitStatus, ChildError> {
        #[cfg(unix)]
        if let ChildScope::Session(session) = &mut self.child {
            return session
                .observe_status()
                .map_err(|source| ChildError::Reap { source });
        }
        let child = self.child.as_mut().ok_or(ChildError::AlreadyReaped)?;
        let status = child
            .wait_status()
            .map_err(|source| ChildError::Reap { source })?;
        self.child.close();
        Ok(status)
    }

    fn wait_with_output(&mut self) -> Result<Output, ChildError> {
        self.wait_with_output_using(Self::wait_for_output)
    }

    fn wait_with_output_using<W>(&mut self, wait: W) -> Result<Output, ChildError>
    where
        W: FnOnce(&mut Self) -> std::io::Result<ExitStatus>,
    {
        let (stdin, stdout, stderr) = match self.child.as_mut() {
            Some(child) => (child.stdin(), child.stdout(), child.stderr()),
            None => return Err(ChildError::AlreadyReaped),
        };
        drop(stdin);

        std::thread::scope(|scope| {
            let stdout = ScopedThread::launch(scope, move || read_pipe(stdout));
            let collecting = Collecting { owner: self };
            let (stderr, collecting) = collecting.start_stderr(scope, stderr);
            let waited = wait(collecting.owner);
            if let Err(wait_error) = &waited
                && let Err(cleanup_error) = collecting.owner.reap()
            {
                terminal_child_ownership_failure(wait_error, &cleanup_error);
            }
            let stdout = collected_pipe(stdout);
            let stderr = collected_pipe(stderr);
            match (waited, stdout, stderr) {
                (Ok(status), Ok(stdout), Ok(stderr)) => Ok(Output {
                    status,
                    stdout,
                    stderr,
                }),
                (wait, stdout, stderr) => {
                    let wait = match wait {
                        Ok(_status) => None,
                        Err(error) => Some(error),
                    };
                    let stdout = match stdout {
                        Ok(_bytes) => None,
                        Err(error) => Some(error),
                    };
                    let stderr = match stderr {
                        Ok(_bytes) => None,
                        Err(error) => Some(error),
                    };
                    Err(ChildError::Output {
                        wait,
                        stdout,
                        stderr,
                    })
                }
            }
        })
    }

    fn wait_for_output(&mut self) -> std::io::Result<ExitStatus> {
        let child = self.child.as_mut().ok_or_else(|| {
            std::io::Error::other("the supervised child was reaped before output collection")
        })?;
        let status = child.wait_status()?;
        self.child.close();
        Ok(status)
    }

    fn reap(&mut self) -> Result<(), ChildError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        child.stop().map_err(|source| ChildError::Reap { source })?;
        self.child.close();
        Ok(())
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        if let Err(source) = self.reap() {
            eprintln!("the supervised child cleanup refused: {source}");
            std::process::abort();
        }
    }
}

struct Collecting<'a> {
    owner: &'a mut ChildOwner,
}

impl Collecting<'_> {
    fn start_stderr<'scope>(
        self,
        scope: &'scope std::thread::Scope<'scope, '_>,
        stderr: Option<ChildStderr>,
    ) -> (ScopedThread<'scope, std::io::Result<Vec<u8>>>, Self) {
        let stderr = ScopedThread::launch(scope, move || read_pipe(stderr));
        (stderr, self)
    }
}

impl Drop for Collecting<'_> {
    fn drop(&mut self) {
        if let Err(source) = self.owner.reap() {
            eprintln!("the supervised output producer cleanup refused: {source}");
            std::process::abort();
        }
    }
}

fn read_pipe<R: std::io::Read>(pipe: Option<R>) -> std::io::Result<Vec<u8>> {
    let Some(mut pipe) = pipe else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn collected_pipe(
    reader: ScopedThread<'_, std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, ProcessOutputError> {
    match reader.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(source)) => Err(ProcessOutputError::read(source)),
        Err(source) => Err(ProcessOutputError::join(source)),
    }
}

#[cold]
fn terminal_child_ownership_failure(wait: &std::io::Error, cleanup: &ChildError) -> ! {
    eprintln!(
        "terminal supervised-child ownership failure: wait failed: {wait}; cleanup failed: {cleanup}"
    );
    std::process::abort();
}

/// What one command said, as a spawned process would have said it.
#[must_use]
pub fn answered(code: u8, out: Vec<u8>, err: Vec<u8>) -> Output {
    Output {
        status: status(code),
        stdout: out,
        stderr: err,
    }
}

/// An exit status that answers `code` to [`std::process::ExitStatus::code`].
#[cfg(unix)]
fn status(code: u8) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    let raw = match i32::from(code).checked_shl(8) {
        Some(raw) => raw,
        None => std::process::abort(),
    };
    ExitStatus::from_raw(raw)
}

/// An exit status that answers `code` to [`std::process::ExitStatus::code`].
#[cfg(windows)]
fn status(code: u8) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt as _;
    ExitStatus::from_raw(u32::from(code))
}

/// The name libtest runs the test `name` under, from the `module_path!()` of the file that declares it: the module path without the crate, so the name is right whether the file is a binary of its own or a module of a crate's one suite.
#[must_use]
pub fn test_name(module: &str, name: &str) -> String {
    match module.split_once("::") {
        Some((_crate, within)) => format!("{within}::{name}"),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::{ChildError, ChildOwner, test_name};

    const OWNERSHIP_CHILD: &str = "NJUTEST_DEVKIT_OWNERSHIP_CHILD";

    #[cfg(unix)]
    #[test]
    fn a_successful_leader_wait_closes_every_inherited_pipe_writer() -> std::io::Result<()> {
        use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
        use std::io::{BufRead as _, Read as _};

        let directory = tempfile::tempdir()?;
        let mut command = Command::new("sh");
        command
            .current_dir(directory.path())
            .args([
                "-c",
                "mkfifo hold; sh -c 'echo writer-ready; read answer < hold' & echo writer-pid=$!",
            ])
            .stdout(Stdio::piped());
        let mut child =
            super::SupervisedChild::launch(&mut command).map_err(std::io::Error::other)?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| std::io::Error::other("missing owned pipe"))?;
        let mut stdout = std::io::BufReader::new(stdout);
        let mut writer = None;
        let mut ready = false;
        while writer.is_none() || !ready {
            let mut line = String::new();
            if stdout.read_line(&mut line)? == 0 {
                return Err(std::io::Error::other(
                    "the writer exited before its readiness event",
                ));
            }
            if let Some(raw) = line.trim_end().strip_prefix("writer-pid=") {
                writer = Some(raw.parse::<u32>().map_err(std::io::Error::other)?);
            }
            if line.trim_end() == "writer-ready" {
                ready = true;
            }
        }
        let writer = njutest_process::ForeignProcess::retain(
            writer
                .ok_or_else(|| std::io::Error::other("the actual inherited writer had no PID"))?,
        )?
        .ok_or_else(|| std::io::Error::other("the inherited writer ended before leader wait"))?;
        child.wait().map_err(std::io::Error::other)?;
        let flags = fcntl_getfl(stdout.get_ref())?;
        fcntl_setfl(stdout.get_ref(), flags | OFlags::NONBLOCK)?;
        let eof = stdout.read(&mut [0_u8; 1]);
        if !matches!(eof, Ok(0)) {
            writer.stop()?;
        }
        if !matches!(eof, Ok(0)) {
            return Err(std::io::Error::other(format!(
                "leader wait returned before the inherited writer closed its pipe: {eof:?}"
            )));
        }
        if !writer.wait(Some(std::time::Duration::ZERO))? {
            return Err(std::io::Error::other(
                "the inherited writer's exact kernel generation remained live after group settlement",
            ));
        }
        Ok(())
    }

    #[test]
    fn an_injected_wait_failure_closes_the_child_before_returning() -> std::io::Result<()> {
        if std::env::var_os(OWNERSHIP_CHILD).is_some() {
            use std::io::Read as _;
            std::io::stdin().read_to_end(&mut Vec::new())?;
            return Ok(());
        }

        let executable = std::env::current_exe()?;
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "process::tests::an_injected_wait_failure_closes_the_child_before_returning",
                "--nocapture",
            ])
            .env(OWNERSHIP_CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut owner = ChildOwner::launch(&mut command).map_err(std::io::Error::other)?;
        let result = owner
            .wait_with_output_using(|_owned| Err(std::io::Error::other("injected wait failure")));

        if owner.live().is_some() {
            return Err(std::io::Error::other(
                "the failure return was reachable before the child had been reaped",
            ));
        }
        match result {
            Err(ChildError::Output {
                wait: Some(_wait),
                stdout: None,
                stderr: None,
            }) => Ok(()),
            other => Err(std::io::Error::other(format!(
                "mandatory cleanup did not retain the exact wait failure: {other:?}"
            ))),
        }
    }

    #[test]
    fn a_test_name_drops_the_crate_and_keeps_every_module_within_it() {
        assert_eq!(test_name("suite::runner", "reaps"), "runner::reaps");
        assert_eq!(test_name("paths", "reaps"), "reaps");
        assert_eq!(
            test_name("njutest_devkit::process::tests", "reaps"),
            "process::tests::reaps"
        );
    }
}
