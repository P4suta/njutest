// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One command's answer, in the shape a spawned one has.

use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output};

use crate::thread::{JoinError, ScopedThread};

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
    /// Waiting or collecting one or both output pipes failed after ownership
    /// had been closed by reaping the child.
    #[error(
        "the supervised child's output could not be collected (wait={wait:?}, stdout={stdout:?}, stderr={stderr:?})"
    )]
    Output {
        /// The failure to wait for the child, when waiting failed.
        wait: Option<std::io::Error>,
        /// The failure to read or join the stdout collector, when it failed.
        stdout: Option<OutputFailure>,
        /// The failure to read or join the stderr collector, when it failed.
        stderr: Option<OutputFailure>,
    },
}

/// Why one owned output-pipe collector could not return all of its bytes.
#[derive(Debug, thiserror::Error)]
#[error("{kind}: {source}")]
pub struct OutputFailure {
    /// Which closed collector transition failed.
    kind: OutputFailureKind,
    /// The underlying read or typed-join failure.
    #[source]
    source: std::io::Error,
}

impl OutputFailure {
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

    /// The operating-system process identifier, while the child is live.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.owner.live().map(Child::id)
    }

    /// Takes the child's standard input pipe, when one was configured.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.owner.live_mut()?.stdin.take()
    }

    /// Takes the child's standard output pipe, when one was configured.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.owner.live_mut()?.stdout.take()
    }

    /// Takes the child's standard error pipe, when one was configured.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.owner.live_mut()?.stderr.take()
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

    /// Waits for the child and collects its configured output pipes.
    ///
    /// # Errors
    /// Returns a typed ownership or operating-system failure.
    pub fn wait_with_output(mut self) -> Result<Output, ChildError> {
        self.owner.wait_with_output()
    }
}

/// The one place a raw `Child` exists. Keeping this type private prevents a
/// caller from separating the handle from its mandatory reap-on-drop policy.
#[derive(Debug)]
struct ChildOwner {
    /// `Some` is the live ownership capability; `None` is reachable only
    /// after a successful wait or reap.
    child: Option<Child>,
}

impl ChildOwner {
    fn launch(command: &mut Command) -> Result<Self, ChildError> {
        command
            .spawn()
            .map(|child| Self { child: Some(child) })
            .map_err(|source| ChildError::Start { source })
    }

    const fn live(&self) -> Option<&Child> {
        self.child.as_ref()
    }

    const fn live_mut(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, ChildError> {
        let child = self.child.as_mut().ok_or(ChildError::AlreadyReaped)?;
        let status = child
            .try_wait()
            .map_err(|source| ChildError::Reap { source })?;
        if status.is_some() {
            self.child = None;
        }
        Ok(status)
    }

    fn wait(&mut self) -> Result<ExitStatus, ChildError> {
        let child = self.child.as_mut().ok_or(ChildError::AlreadyReaped)?;
        let status = child.wait().map_err(|source| ChildError::Reap { source })?;
        self.child = None;
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
            Some(child) => (child.stdin.take(), child.stdout.take(), child.stderr.take()),
            None => return Err(ChildError::AlreadyReaped),
        };
        drop(stdin);

        std::thread::scope(|scope| {
            let stdout = ScopedThread::launch(scope, move || read_pipe(stdout));
            let stderr = ScopedThread::launch(scope, move || read_pipe(stderr));
            let waited = wait(self);
            if let Err(wait_error) = &waited
                && let Err(cleanup_error) = self.reap()
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
        let status = child.wait()?;
        self.child = None;
        Ok(status)
    }

    fn reap(&mut self) -> Result<(), ChildError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        match child.try_wait() {
            Ok(Some(_status)) => {
                self.child = None;
                return Ok(());
            }
            Ok(None) => {}
            Err(source) => return Err(ChildError::Reap { source }),
        }
        if let Err(source) = child.kill()
            && child
                .try_wait()
                .map_err(|observe| ChildError::Reap { source: observe })?
                .is_none()
        {
            return Err(ChildError::Reap { source });
        }
        self.wait().map(|_status| ())
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.reap().is_err() {
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
) -> Result<Vec<u8>, OutputFailure> {
    match reader.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(source)) => Err(OutputFailure::read(source)),
        Err(source) => Err(OutputFailure::join(source)),
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

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::{ChildError, ChildOwner};

    const OWNERSHIP_CHILD: &str = "NJUTEST_DEVKIT_OWNERSHIP_CHILD";

    #[test]
    fn an_injected_wait_failure_closes_the_child_before_returning() -> std::io::Result<()> {
        if std::env::var_os(OWNERSHIP_CHILD).is_some() {
            std::thread::sleep(Duration::from_secs(60));
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut owner = ChildOwner::launch(&mut command).map_err(std::io::Error::other)?;
        let result = owner
            .wait_with_output_using(|_owned| Err(std::io::Error::other("injected wait failure")));

        if owner.child.is_some() {
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
}
