// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Talking to a provider: one process, newline-delimited strict JSON.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{self, ErrorCode};

/// The protocol version this release speaks.
pub const VERSION: u32 = 1;

/// The most a provider may say in one line.
pub const LINE_LIMIT: usize = 4 << 20;

/// One pending provider answer is enough: the protocol permits only one outstanding request.
const ANSWER_CAPACITY: usize = 1;

/// The largest opaque instance identity accepted from a provider.
const INSTANCE_ID_LIMIT: usize = 4 << 10;

/// What one request asks a resource provider to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Start an instance of the capability.
    Start,
    /// Stop the instance named in the request.
    Stop,
}

/// The identity that pairs one request with its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RequestId(String);

impl RequestId {
    fn for_sequence(sequence: u32) -> Self {
        Self(format!("resource-{sequence:06}"))
    }
}

/// Why an opaque provider instance identity is not safe to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InstanceIdError {
    /// Empty identities cannot correlate a later stop request.
    #[error("a provider instance identity is empty")]
    Empty,
    /// A bounded protocol value must not become an unbounded allocation.
    #[error("a provider instance identity is {bytes} bytes; the limit is {INSTANCE_ID_LIMIT}")]
    TooLong {
        /// The number of bytes the provider supplied.
        bytes: usize,
    },
    /// Control characters make the identity unsafe in diagnostics and line protocols.
    #[error("a provider instance identity contains a control character at byte {byte}")]
    Control {
        /// The byte offset of the first control character.
        byte: usize,
    },
}

/// An opaque, non-empty provider instance identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    /// Validates an instance identity at the protocol boundary.
    ///
    /// # Errors
    /// [`InstanceIdError`] when the value is empty, unbounded, or contains a control character.
    pub fn checked(value: impl Into<String>) -> Result<Self, InstanceIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(InstanceIdError::Empty);
        }
        if value.len() > INSTANCE_ID_LIMIT {
            return Err(InstanceIdError::TooLong { bytes: value.len() });
        }
        if let Some((byte, _control)) = value.char_indices().find(|(_byte, ch)| ch.is_control()) {
            return Err(InstanceIdError::Control { byte });
        }
        Ok(Self(value))
    }

    /// The validated protocol spelling.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for InstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::checked(value).map_err(serde::de::Error::custom)
    }
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
    version: u32,
    action: Action,
    capability: String,
    #[serde(rename = "request_id")]
    id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<InstanceId>,
}

impl Request {
    /// The request that starts an instance of `capability`.
    #[must_use]
    pub fn start(capability: &str, sequence: u32) -> Self {
        Self {
            version: VERSION,
            action: Action::Start,
            capability: capability.to_owned(),
            id: RequestId::for_sequence(sequence),
            instance: None,
        }
    }

    /// The request that stops `instance`.
    #[must_use]
    pub fn stop(capability: &str, instance: &InstanceId, sequence: u32) -> Self {
        Self {
            version: VERSION,
            action: Action::Stop,
            capability: capability.to_owned(),
            id: RequestId::for_sequence(sequence),
            instance: Some(instance.clone()),
        }
    }

    fn capability(&self) -> &str {
        &self.capability
    }
}

/// The closed response union spoken on the wire.
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
enum ResponseWire {
    Ready {
        version: u32,
        instance: InstanceId,
        environment: BTreeMap<String, String>,
    },
    Stopped {
        version: u32,
        instance: InstanceId,
    },
    Error {
        version: u32,
        message: String,
    },
}

impl ResponseWire {
    const fn version(&self) -> u32 {
        match self {
            Self::Ready { version, .. }
            | Self::Stopped { version, .. }
            | Self::Error { version, .. } => *version,
        }
    }

    const fn status(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::Stopped { .. } => "stopped",
            Self::Error { .. } => "error",
        }
    }
}

/// A successful provider answer whose action-specific invariants were checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    instance: InstanceId,
    environment: BTreeMap<String, String>,
}

impl Response {
    /// Consumes the answer into its validated instance and environment.
    #[must_use]
    pub fn into_parts(self) -> (InstanceId, BTreeMap<String, String>) {
        (self.instance, self.environment)
    }
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
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

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    const fn is_timeout(&self) -> bool {
        matches!(self.kind, ProviderErrorKind::Timeout)
    }

    pub(crate) fn with_cleanup(self, cleanup: Self) -> Self {
        let Self { kind, message } = self;
        let Self {
            message: cleanup_message,
            ..
        } = cleanup;
        Self::new(
            kind,
            format!("{message}; cleanup also failed: {cleanup_message}"),
        )
    }
}

/// An owned child process that kills and reaps itself even on an early return.
#[derive(Debug)]
struct SupervisedChild {
    child: Child,
    reaped: bool,
}

impl SupervisedChild {
    fn launch(command: &mut Command) -> std::io::Result<Self> {
        command.spawn().map(|child| Self {
            child,
            reaped: false,
        })
    }

    const fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    const fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }

    fn try_wait(&mut self) -> std::io::Result<bool> {
        match self.child.try_wait()? {
            Some(_status) => {
                self.reaped = true;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn wait(&mut self) -> std::io::Result<()> {
        self.child.wait().map(|_status| {
            self.reaped = true;
        })
    }

    fn terminate_and_wait(&mut self) -> std::io::Result<()> {
        let killed = kill_tree(&mut self.child);
        let waited = self.wait();
        match (killed, waited) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(kill), Ok(())) => Err(kill),
            (Ok(()), Err(wait)) => Err(wait),
            (Err(kill), Err(wait)) => Err(std::io::Error::new(
                wait.kind(),
                format!("cannot kill the provider tree: {kill}; cannot reap it: {wait}"),
            )),
        }
    }

    fn finish(&mut self, timeout: Duration) -> std::io::Result<()> {
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the provider shutdown deadline is outside Instant's range",
            )
        })?;
        loop {
            match self.try_wait() {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(poll) => {
                    return match self.terminate_and_wait() {
                        Ok(()) => Err(poll),
                        Err(cleanup) => Err(std::io::Error::new(
                            poll.kind(),
                            format!(
                                "cannot inspect the provider: {poll}; cleanup also failed: {cleanup}"
                            ),
                        )),
                    };
                }
            }
            if Instant::now() >= deadline {
                return self.terminate_and_wait();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if let Err(cleanup) = self.terminate_and_wait() {
            drop(cleanup);
        }
    }
}

/// A join handle whose destructor cannot detach its thread.
#[derive(Debug)]
struct JoinedThread {
    handle: Option<JoinHandle<()>>,
}

/// A one-shot provider and the reader whose lifetime it owns.
#[derive(Debug)]
struct OneShot {
    child: SupervisedChild,
    answer: Option<Receiver<ReadAnswer>>,
    reader: Option<JoinedThread>,
}

impl OneShot {
    fn receive(&self, timeout: Duration) -> Result<String, ProviderError> {
        let answer = self.answer.as_ref().ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider output reader is no longer available",
            )
        })?;
        match answer.recv_timeout(timeout) {
            Ok(Ok(all)) => Ok(all),
            Ok(Err(source)) => Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                source.to_string(),
            )),
            Err(RecvTimeoutError::Timeout) => Err(ProviderError::new(
                ProviderErrorKind::Timeout,
                format!(
                    "the provider did not end in {}",
                    rust_mutants::duration::render(timeout)
                ),
            )),
            Err(RecvTimeoutError::Disconnected) => Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider output reader ended without an answer",
            )),
        }
    }

    fn finish(mut self, timeout: Duration, terminate: bool) -> Result<(), ProviderError> {
        let answer = self.answer.take();
        drop(answer);
        let process = if terminate {
            self.child.terminate_and_wait()
        } else {
            self.child.finish(timeout)
        }
        .map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("cannot end the provider process: {source}"),
            )
        });
        let reader = match self.reader.take() {
            Some(reader) => reader.join(),
            None => Ok(()),
        };
        match (process, reader) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(process), Ok(())) => Err(process),
            (Ok(()), Err(reader)) => Err(reader),
            (Err(process), Err(reader)) => Err(process.with_cleanup(reader)),
        }
    }
}

impl JoinedThread {
    fn launch(name: &'static str, work: impl FnOnce() + Send + 'static) -> std::io::Result<Self> {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(work)
            .map(|handle| Self {
                handle: Some(handle),
            })
    }

    fn join(mut self) -> Result<(), ProviderError> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle.join().map_err(|_panic| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider output reader panicked",
            )
        })
    }
}

impl Drop for JoinedThread {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if let Err(panic) = handle.join() {
            drop(panic);
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ReadFailure {
    #[error("cannot read what the provider said: {0}")]
    Io(#[from] std::io::Error),
    #[error("the provider wrote more than {limit} bytes")]
    TooLong { limit: usize },
    #[error("the provider output is not UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

type ReadAnswer = Result<String, ReadFailure>;

fn send_answer(sender: &SyncSender<ReadAnswer>, answer: ReadAnswer) -> bool {
    match sender.send(answer) {
        Ok(()) => true,
        Err(_closed) => false,
    }
}

fn spawn_line_reader(
    stdout: std::process::ChildStdout,
) -> Result<(Receiver<ReadAnswer>, JoinedThread), ProviderError> {
    let limit = u64::try_from(LINE_LIMIT)
        .map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("the provider line limit cannot be represented: {source}"),
            )
        })?
        .checked_add(1)
        .ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider line limit cannot be incremented",
            )
        })?;
    let (sender, lines) = sync_channel(ANSWER_CAPACITY);
    let reader = JoinedThread::launch("njutest-provider-lines", move || {
        let mut input = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            let read = input.by_ref().take(limit).read_until(b'\n', &mut bytes);
            match read {
                Ok(0) => return,
                Ok(_read) if bytes.len() > LINE_LIMIT => {
                    let sent =
                        send_answer(&sender, Err(ReadFailure::TooLong { limit: LINE_LIMIT }));
                    if !sent {
                        return;
                    }
                    return;
                }
                Ok(_read) => {
                    let answer = String::from_utf8(bytes).map_err(ReadFailure::from);
                    let terminal = answer.is_err();
                    if !send_answer(&sender, answer) || terminal {
                        return;
                    }
                }
                Err(source) => {
                    let sent = send_answer(&sender, Err(ReadFailure::Io(source)));
                    if !sent {
                        return;
                    }
                    return;
                }
            }
        }
    })
    .map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Unstartable,
            format!("cannot start the provider output reader: {source}"),
        )
    })?;
    Ok((lines, reader))
}

fn spawn_all_reader(
    stdout: std::process::ChildStdout,
    limit: usize,
) -> Result<(Receiver<ReadAnswer>, JoinedThread), ProviderError> {
    let capacity = u64::try_from(limit)
        .map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("the provider output limit cannot be represented: {source}"),
            )
        })?
        .checked_add(1)
        .ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider output limit cannot be incremented",
            )
        })?;
    let (sender, answer) = sync_channel(ANSWER_CAPACITY);
    let reader = JoinedThread::launch("njutest-provider-output", move || {
        let mut bytes = Vec::new();
        let read = BufReader::new(stdout)
            .take(capacity)
            .read_to_end(&mut bytes);
        let answer = match read {
            Ok(_read) if bytes.len() > limit => Err(ReadFailure::TooLong { limit }),
            Ok(_read) => String::from_utf8(bytes).map_err(ReadFailure::from),
            Err(source) => Err(ReadFailure::Io(source)),
        };
        match sender.send(answer) {
            Ok(()) => {}
            Err(closed) => drop(closed),
        }
    })
    .map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Unstartable,
            format!("cannot start the provider output reader: {source}"),
        )
    })?;
    Ok((answer, reader))
}

/// One running provider process, and the line reader that keeps a slow answer from blocking the run.
#[derive(Debug)]
pub struct Process {
    child: SupervisedChild,
    stdin: Option<ChildStdin>,
    lines: Option<Receiver<ReadAnswer>>,
    reader: Option<JoinedThread>,
}

impl Process {
    /// Starts `command` in `dir` with exactly `env`.
    ///
    /// # Errors
    /// [`ProviderErrorKind::Unstartable`] when the command is empty or the operating system refuses it.
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
        let mut child = SupervisedChild::launch(&mut spawning).map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Unstartable,
                format!("cannot start {program}: {source}"),
            )
        })?;
        let stdin = child.take_stdin();
        let stdout = child.take_stdout().ok_or_else(|| {
            ProviderError::new(ProviderErrorKind::Unstartable, "the provider has no stdout")
        })?;
        let (lines, reader) = spawn_line_reader(stdout)?;
        Ok(Self {
            child,
            stdin,
            lines: Some(lines),
            reader: Some(reader),
        })
    }

    /// Asks one question and reads one answer.
    ///
    /// # Errors
    /// [`ProviderErrorKind::Protocol`] for a line that is not this version's answer, and [`ProviderErrorKind::Timeout`] for no line at all.
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
        let lines = self.lines.as_ref().ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                "the provider output reader is no longer available",
            )
        })?;
        let said = match lines.recv_timeout(timeout) {
            Ok(Ok(said)) => said,
            Ok(Err(source)) => {
                return Err(ProviderError::new(
                    ProviderErrorKind::Protocol,
                    source.to_string(),
                ));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(ProviderError::new(
                    ProviderErrorKind::Timeout,
                    format!(
                        "the provider said nothing about {} in {}",
                        request.capability(),
                        rust_mutants::duration::render(timeout)
                    ),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ProviderError::new(
                    ProviderErrorKind::Protocol,
                    format!(
                        "the provider ended without answering about {}",
                        request.capability()
                    ),
                ));
            }
        };
        read(&said, request)
    }

    /// Closes the conversation, waits for the process, and ends its whole tree if it stays.
    ///
    /// # Errors
    /// [`ProviderErrorKind::Protocol`] when the process cannot be inspected,
    /// killed, reaped, or its owned reader thread cannot be joined.
    pub fn end(mut self, timeout: Duration) -> Result<(), ProviderError> {
        let stdin = self.stdin.take();
        drop(stdin);
        let lines = self.lines.take();
        drop(lines);
        let process = self.child.finish(timeout).map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("cannot end the provider process: {source}"),
            )
        });
        let reader = match self.reader.take() {
            Some(reader) => reader.join(),
            None => Ok(()),
        };
        match (process, reader) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(process), Ok(())) => Err(process),
            (Ok(()), Err(reader)) => Err(reader),
            (Err(process), Err(reader)) => Err(process.with_cleanup(reader)),
        }
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
    let Some(body) = said.strip_suffix('\n') else {
        return Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            "the provider response is not terminated by a newline",
        ));
    };
    let body = match body.strip_suffix('\r') {
        Some(without_carriage_return) => without_carriage_return,
        None => body,
    };
    let response: ResponseWire = crate::strictjson::decode_str(body).map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Protocol,
            format!("the provider said something that is not this protocol: {source}"),
        )
    })?;
    if response.version() != VERSION {
        return Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            format!(
                "the provider answered in version {} and this release speaks {VERSION}",
                response.version()
            ),
        ));
    }
    match (request.action, response) {
        (
            Action::Start,
            ResponseWire::Ready {
                instance,
                environment,
                ..
            },
        ) => Ok(Response {
            instance,
            environment,
        }),
        (Action::Stop, ResponseWire::Stopped { instance, .. }) => match request.instance.as_ref() {
            Some(wanted) if wanted == &instance => Ok(Response {
                instance,
                environment: BTreeMap::new(),
            }),
            Some(wanted) => Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                format!(
                    "the provider stopped instance {instance:?} after it was asked to stop {wanted:?}"
                ),
            )),
            None => Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                "an internal stop request did not name its instance",
            )),
        },
        (_action, ResponseWire::Error { message, .. }) => Err(ProviderError::new(
            ProviderErrorKind::Refused,
            format!(
                "the provider refused {} of {}: {message}",
                request.action.name(),
                request.capability()
            ),
        )),
        (action, other) => Err(ProviderError::new(
            ProviderErrorKind::Protocol,
            format!(
                "the provider answered {} to {} of {}",
                other.status(),
                action.name(),
                request.capability()
            ),
        )),
    }
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
/// [`ProviderErrorKind::Timeout`] when it does not end in time, and [`ProviderErrorKind::Protocol`] when it writes more than `limit`.
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
    let mut child = SupervisedChild::launch(&mut spawning).map_err(|source| {
        ProviderError::new(
            ProviderErrorKind::Unstartable,
            format!("cannot start {program}: {source}"),
        )
    })?;
    let mut stdin = child.take_stdin().ok_or_else(|| {
        ProviderError::new(
            ProviderErrorKind::Unstartable,
            "the provider has no standard input",
        )
    })?;
    let mut asked = question.to_owned();
    asked.push('\n');
    stdin
        .write_all(asked.as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|source| {
            ProviderError::new(
                ProviderErrorKind::Protocol,
                format!("cannot ask the provider: {source}"),
            )
        })?;
    drop(stdin);
    let stdout = child.take_stdout().ok_or_else(|| {
        ProviderError::new(ProviderErrorKind::Unstartable, "the provider has no stdout")
    })?;
    let (said, reader) = spawn_all_reader(stdout, limit)?;
    let running = OneShot {
        child,
        answer: Some(said),
        reader: Some(reader),
    };
    let answer = running.receive(timeout);
    let timed_out = match &answer {
        Ok(_answer) => false,
        Err(error) => error.is_timeout(),
    };
    let cleanup = running.finish(timeout, timed_out);
    match (answer, cleanup) {
        (Ok(answer), Ok(())) => Ok(answer),
        (Err(answer), Ok(())) => Err(answer),
        (Ok(_answer), Err(cleanup)) => Err(cleanup),
        (Err(answer), Err(cleanup)) => Err(answer.with_cleanup(cleanup)),
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
fn kill_tree(child: &mut Child) -> std::io::Result<()> {
    let Ok(raw) = i32::try_from(child.id()) else {
        return child.kill();
    };
    let Some(pid) = rustix::process::Pid::from_raw(raw) else {
        return child.kill();
    };
    match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(source) => Err(std::io::Error::from(source)),
    }
}

#[cfg(not(unix))]
fn kill_tree(child: &mut Child) -> std::io::Result<()> {
    child.kill()
}

#[cfg(test)]
mod tests {
    use super::{InstanceId, ProviderErrorKind, Request, read};

    fn protocol_error(document: &str, request: &Request) {
        let refusal = read(document, request).expect_err("protocol must be refused");
        assert_eq!(refusal.kind, ProviderErrorKind::Protocol);
    }

    #[test]
    fn ready_is_a_closed_action_specific_shape() {
        let request = Request::start("postgres", 1);
        let answered = read(
            "{\"version\":1,\"status\":\"ready\",\"instance\":\"pg-1\",\"environment\":{}}\n",
            &request,
        )
        .expect("ready");
        let (instance, environment) = answered.into_parts();
        assert_eq!(instance.as_str(), "pg-1");
        assert!(environment.is_empty());

        protocol_error(
            "{\"version\":1,\"status\":\"ready\",\"instance\":\"pg-1\"}\n",
            &request,
        );
        protocol_error(
            "{\"version\":1,\"status\":\"ready\",\"instance\":\"pg-1\",\"environment\":{},\"message\":null}\n",
            &request,
        );
    }

    #[test]
    fn stopped_must_name_exactly_the_instance_requested() {
        let instance = InstanceId::checked("pg-1").expect("instance id");
        let request = Request::stop("postgres", &instance, 2);
        read(
            "{\"version\":1,\"status\":\"stopped\",\"instance\":\"pg-1\"}\n",
            &request,
        )
        .expect("stopped");
        protocol_error(
            "{\"version\":1,\"status\":\"stopped\",\"instance\":\"another\"}\n",
            &request,
        );
        protocol_error(
            "{\"version\":1,\"status\":\"stopped\",\"instance\":\"pg-1\",\"environment\":{}}\n",
            &request,
        );
    }

    #[test]
    fn response_rejects_unknown_duplicate_missing_and_unframed_input() {
        let request = Request::start("postgres", 1);
        protocol_error(
            "{\"version\":1,\"status\":\"future\",\"instance\":\"pg-1\",\"environment\":{}}\n",
            &request,
        );
        protocol_error(
            "{\"version\":1,\"version\":1,\"status\":\"ready\",\"instance\":\"pg-1\",\"environment\":{}}\n",
            &request,
        );
        protocol_error(
            "{\"version\":1,\"status\":\"ready\",\"instance\":\"\",\"environment\":{}}\n",
            &request,
        );
        protocol_error(
            "{\"version\":1,\"status\":\"ready\",\"instance\":\"pg-1\",\"environment\":{}}",
            &request,
        );
    }

    #[test]
    fn refusal_is_a_distinct_closed_variant_with_a_required_reason() {
        let request = Request::start("postgres", 1);
        let refusal = read(
            "{\"version\":1,\"status\":\"error\",\"message\":\"no docker\"}\n",
            &request,
        )
        .expect_err("refused");
        assert_eq!(refusal.kind, ProviderErrorKind::Refused);
        protocol_error("{\"version\":1,\"status\":\"error\"}\n", &request);
    }
}
