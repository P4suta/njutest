// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Talking to a provider: one process, newline-delimited strict JSON.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{self, ErrorCode};

/// The protocol version this release speaks.
pub const VERSION: u32 = 1;

/// The most a provider may say in one line.
pub const LINE_LIMIT: usize = 4 << 20;

/// What one request asks a resource provider to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Start an instance of the capability.
    Start,
    /// Stop the instance named in the request.
    Stop,
}

impl Action {
    /// The word the protocol uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

/// One request to a resource provider.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// The protocol version.
    pub version: u32,
    /// What to do.
    pub action: String,
    /// The capability the resource provides.
    pub capability: String,
    /// This request's identity, which the answer carries back.
    #[serde(rename = "request_id")]
    pub id: String,
    /// The instance to stop; absent when starting one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl Request {
    /// The request that starts an instance of `capability`.
    #[must_use]
    pub fn start(capability: &str, sequence: u32) -> Self {
        Self {
            version: VERSION,
            action: Action::Start.name().to_owned(),
            capability: capability.to_owned(),
            id: format!("resource-{sequence:06}"),
            instance: None,
        }
    }

    /// The request that stops `instance`.
    #[must_use]
    pub fn stop(capability: &str, instance: &str, sequence: u32) -> Self {
        Self {
            version: VERSION,
            action: Action::Stop.name().to_owned(),
            capability: capability.to_owned(),
            id: format!("resource-{sequence:06}"),
            instance: Some(instance.to_owned()),
        }
    }
}

/// What a resource provider answered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// The protocol version, which must be the one asked in.
    pub version: u32,
    /// `ready`, `stopped`, or `error`.
    pub status: String,
    /// The instance the provider started or stopped.
    #[serde(default)]
    pub instance: Option<String>,
    /// What the tests of a leasing run see in their environment.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// What went wrong, when the status says something did.
    #[serde(default)]
    pub message: Option<String>,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderErrorKind {
    /// The provider could not be started.
    Unstartable,
    /// The provider said nothing in the time it was given.
    Timeout,
    /// The provider said something this version does not understand.
    Protocol,
    /// The provider said it could not do it.
    Refused,
}

impl ProviderErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 4] = [
        Self::Unstartable,
        Self::Timeout,
        Self::Protocol,
        Self::Refused,
    ];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Unstartable => error::PROVIDER_UNSTARTABLE,
            Self::Timeout => error::PROVIDER_TIMEOUT,
            Self::Protocol => error::PROVIDER_PROTOCOL,
            Self::Refused => error::PROVIDER_REFUSED,
        }
    }
}

/// Why a provider could not be used.
#[derive(Debug, thiserror::Error)]
#[error("{}: {message}", kind.code().code)]
pub struct ProviderError {
    kind: ProviderErrorKind,
    message: String,
}

impl ProviderError {
    /// A failure of `kind` with `message`.
    #[must_use]
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> ProviderErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

/// One running provider process, and the line reader that keeps a slow answer from blocking the run.
#[derive(Debug)]
pub struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl Process {
    /// Starts `command` in `dir` with exactly `env`.
    ///
    /// # Errors
    /// [`ProviderErrorKind::Unstartable`] when the command is empty or the
    /// operating system refuses it.
    pub fn start(
        command: &[String],
        dir: &Path,
        env: &[(OsString, OsString)],
    ) -> Result<Self, ProviderError> {
        let Some((program, arguments)) = command.split_first() else {
            return Err(ProviderError::new(
                ProviderErrorKind::Unstartable,
                "a provider with no command to run",
            ));
        };
        let mut spawning = Command::new(program);
        spawning
            .args(arguments)
            .current_dir(dir)
            .env_clear()
            .envs(env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        grouped(&mut spawning);
        let mut child = spawning.spawn().map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Unstartable,
                format!("cannot start {program}: {source}"),
            )
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| {
            ProviderError::new(ProviderErrorKind::Unstartable, "the provider has no stdout")
        })?;
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
        })
    }

    /// Asks one question and reads one answer.
    ///
    /// # Errors
    /// [`ProviderErrorKind::Protocol`] for a line that is not this version's
    /// answer, and [`ProviderErrorKind::Timeout`] for no line at all.
    pub fn ask(&mut self, request: &Request, timeout: Duration) -> Result<Response, ProviderError> {
        let mut line = serde_json::to_string(request).map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("cannot write the request: {source}"),
            )
        })?;
        line.push('\n');
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider is no longer listening",
            )
        })?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|source| {
                ProviderError::new(
                    ProviderErrorKind::Protocol,
                    format!("cannot reach the provider: {source}"),
                )
            })?;
        let said = match self.lines.recv_timeout(timeout) {
            Ok(said) => said,
            Err(RecvTimeoutError::Timeout) => {
                return Err(ProviderError::new(
                    ProviderErrorKind::Timeout,
                    format!(
                        "the provider said nothing about {} in {}",
                        request.capability,
                        rust_mutants::duration::render(timeout)
                    ),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ProviderError::new(
                    ProviderErrorKind::Protocol,
                    format!(
                        "the provider ended without answering about {}",
                        request.capability
                    ),
                ));
            }
        };
        read(&said, request)
    }

    /// Closes the conversation, waits for the process, and ends its whole tree if it stays.
    pub fn end(mut self, timeout: Duration) {
        drop(self.stdin.take());
        let deadline = std::time::Instant::now().checked_add(timeout);
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return,
                Ok(None) => {}
                Err(_) => break,
            }
            if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        kill_tree(&mut self.child);
        let _waited = self.child.wait();
    }
}

/// Reads one answer, refusing anything this version does not understand.
fn read(said: &str, request: &Request) -> Result<Response, ProviderError> {
    if said.len() > LINE_LIMIT {
        return Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            format!("the provider said more than {LINE_LIMIT} bytes in one line"),
        ));
    }
    let response: Response = serde_json::from_str(said.trim_end()).map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Protocol,
            format!("the provider said something that is not this protocol: {source}"),
        )
    })?;
    if response.version != VERSION {
        return Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            format!(
                "the provider answered in version {} and this release speaks {VERSION}",
                response.version
            ),
        ));
    }
    let wanted = if request.instance.is_some() {
        "stopped"
    } else {
        "ready"
    };
    if response.status != wanted {
        return Err(ProviderError::new(
            ProviderErrorKind::Refused,
            format!(
                "the provider answered {:?} to {} of {}: {}",
                response.status,
                request.action,
                request.capability,
                response.message.as_deref().unwrap_or("no reason given")
            ),
        ));
    }
    if response.instance.as_ref().is_none_or(String::is_empty) {
        return Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            format!(
                "the provider answered about {} without naming an instance",
                request.capability
            ),
        ));
    }
    Ok(response)
}

/// One question for a provider that answers once and ends.
#[derive(Debug, Clone, Copy)]
pub struct Once<'a> {
    /// The command to run.
    pub command: &'a [String],
    /// The directory it runs in.
    pub dir: &'a Path,
    /// The environment it runs with.
    pub env: &'a [(OsString, OsString)],
    /// What it is asked, which goes to its standard input.
    pub question: &'a str,
    /// How long it may take to end.
    pub timeout: Duration,
    /// The most it may say.
    pub limit: usize,
}

/// Asks one provider one question and reads everything it says back, once.
///
/// # Errors
/// [`ProviderErrorKind::Unstartable`] when the command cannot be run,
/// [`ProviderErrorKind::Timeout`] when it does not end in time, and
/// [`ProviderErrorKind::Protocol`] when it writes more than `limit`.
pub fn once(asking: &Once<'_>) -> Result<String, ProviderError> {
    let Once {
        command,
        dir,
        env,
        question,
        timeout,
        limit,
    } = *asking;
    let Some((program, arguments)) = command.split_first() else {
        return Err(ProviderError::new(
            ProviderErrorKind::Unstartable,
            "a provider with no command to run",
        ));
    };
    let mut spawning = Command::new(program);
    spawning
        .args(arguments)
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    grouped(&mut spawning);
    let mut child = spawning.spawn().map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Unstartable,
            format!("cannot start {program}: {source}"),
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut asked = question.to_owned();
        asked.push('\n');
        let _written = stdin
            .write_all(asked.as_bytes())
            .and_then(|()| stdin.flush());
    }
    let stdout = child.stdout.take().ok_or_else(|| {
        ProviderError::new(ProviderErrorKind::Unstartable, "the provider has no stdout")
    })?;
    let (sender, said) = channel();
    std::thread::spawn(move || {
        use std::io::Read as _;

        let mut all = String::new();
        let mut reader = BufReader::new(stdout);
        let read = reader.read_to_string(&mut all);
        let _sent = sender.send(read.map(|_read| all));
    });
    match said.recv_timeout(timeout) {
        Ok(Ok(all)) if all.len() > limit => {
            kill_tree(&mut child);
            let _waited = child.wait();
            Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("the provider wrote more than {limit} bytes"),
            ))
        }
        Ok(Ok(all)) => {
            let _waited = child.wait();
            Ok(all)
        }
        Ok(Err(source)) => {
            let _waited = child.wait();
            Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("cannot read what the provider said: {source}"),
            ))
        }
        Err(_elapsed) => {
            kill_tree(&mut child);
            let _waited = child.wait();
            Err(ProviderError::new(
                ProviderErrorKind::Timeout,
                format!(
                    "the provider did not end in {}",
                    rust_mutants::duration::render(timeout)
                ),
            ))
        }
    }
}

#[cfg(unix)]
fn grouped(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
const fn grouped(_command: &mut Command) {}

#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    let Ok(raw) = i32::try_from(child.id()) else {
        let _killed = child.kill();
        return;
    };
    let Some(pid) = rustix::process::Pid::from_raw(raw) else {
        let _killed = child.kill();
        return;
    };
    let _sent = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    let _killed = child.kill();
}

#[cfg(not(unix))]
fn kill_tree(child: &mut Child) {
    let _killed = child.kill();
}
