// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Actual output arrivals and measured receipts for the existing owned command runner.

use std::io::{self, Read as _, Seek as _, Write as _};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Instant;

use super::{Output, Recipient, Request};
use crate::environment::Environment;
use crate::work::{self, Ended, Stops, WorkError, WorkEvents};

/// Accepts only the explicitly selected inherited protocol before any command or probe starts.
#[cfg_attr(
    not(unix),
    expect(
        unused_variables,
        reason = "only the native Unix owner of an inherited original session measures its admission against the retained stops"
    )
)]
pub(super) fn recipient(environment: &Environment, stops: &Stops) -> io::Result<Recipient> {
    match environment.value("NJUTEST_SESSION_CUSTODY") {
        None => Ok(Recipient::Direct),
        Some(value) if value == "stdin-v1" => {
            #[cfg(unix)]
            {
                let machine = crate::observation::Machine {
                    os: std::env::consts::OS,
                    cpus: std::thread::available_parallelism()?.get(),
                };
                let began = Instant::now();
                let parent = njutest_process::ParentSession::accept_stdin();
                let elapsed = u64::try_from(began.elapsed().as_nanos());
                match (parent, elapsed) {
                    (Ok(parent), Ok(elapsed_ns)) => {
                        stops.record(crate::observation::WaitNote {
                            owner: format!("xtask-original-session:{}", std::process::id()),
                            cause: "native-parent-handshake-identity-and-session".to_owned(),
                            elapsed_ns,
                            machine,
                        });
                        Ok(Recipient::Original { parent })
                    }
                    (Err(source), Ok(elapsed_ns)) => {
                        stops.record(crate::observation::WaitNote {
                            owner: format!("xtask-original-session:{}", std::process::id()),
                            cause: "native-parent-handshake-identity-and-session".to_owned(),
                            elapsed_ns,
                            machine,
                        });
                        Err(source)
                    }
                    (Ok(_parent), Err(source)) => Err(io::Error::other(source)),
                    (Err(parent), Err(measurement)) => Err(io::Error::other(format!(
                        "native custody failed: {parent}; admission measurement failed: {measurement}"
                    ))),
                }
            }
            #[cfg(not(unix))]
            {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "the inherited original-session protocol requires its native Unix owner",
                ))
            }
        }
        Some(value) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the original-session custody protocol is not supported: {}",
                value.display()
            ),
        )),
    }
}

/// Captures actual bytes before notifying the work observer and publishes every result's waits.
pub(super) fn run<F>(request: Request<'_, '_>, started: F) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> io::Result<()>,
{
    run_with_recipient(request, started, Recipient::Direct)
}

/// Runs with the actual pre-admitted recipient while retaining every original output and receipt.
pub(super) fn run_with_recipient<F>(
    request: Request<'_, '_>,
    started: F,
    recipient: Recipient,
) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> io::Result<()>,
{
    let Request {
        command,
        bound,
        stops,
        environment,
        output,
    } = request;
    command.env_remove("NJUTEST_SESSION_CUSTODY");
    let began = Instant::now();
    let invocation = super::hostcost::Invocation::of(command);
    let events = stops.events();
    let leader = std::cell::Cell::new(None);
    let (ran, joined) = match RunningOutput::launch(command, output, &events) {
        Ok(mut reading) => {
            let callback = |pid| {
                leader.set(Some(pid));
                started(pid)
            };
            let ran = match recipient {
                Recipient::Direct => work::run(reading.command, bound, stops, callback),
                #[cfg(unix)]
                Recipient::Original { parent } => match reading.custody(parent) {
                    Ok(custody) => work::run_with_custody(
                        work::Request {
                            command: reading.command,
                            bound,
                            stops,
                            custody,
                        },
                        callback,
                    ),
                    Err(source) => Err(WorkError::Watch { source }),
                },
            };
            (ran, reading.finish())
        }
        Err(source) => (Err(WorkError::Watch { source }), Ok(())),
    };
    let ran = finished(ran, joined);
    let waited = stops.take_waits();
    let published = super::hostcost::publish(
        super::hostcost::Measured {
            invocation,
            leader: leader.get(),
            began,
            outcome: &ran,
        },
        waited,
        environment,
    );
    finished(ran, published)
}

#[derive(Debug)]
struct RunningOutput<'a> {
    command: &'a mut Command,
    pipes: Option<Pipes>,
}

impl<'a> RunningOutput<'a> {
    fn launch(command: &'a mut Command, output: Output, events: &WorkEvents) -> io::Result<Self> {
        let pipes = Pipes::launch(command, output, events)?;
        Ok(Self {
            command,
            pipes: Some(pipes),
        })
    }

    #[cfg(unix)]
    fn custody(&mut self, parent: njutest_process::ParentSession) -> io::Result<work::Custody> {
        let pipes = self
            .pipes
            .as_mut()
            .ok_or_else(|| io::Error::other("the original output owner was already consumed"))?;
        let stdout = pipes.stdout.endpoint.take().ok_or_else(|| {
            io::Error::other("the original standard-output endpoint was already consumed")
        })?;
        let stderr = pipes.stderr.endpoint.take().ok_or_else(|| {
            io::Error::other("the original standard-error endpoint was already consumed")
        })?;
        Ok(work::Custody::Original {
            parent,
            stdout,
            stderr,
        })
    }

    fn finish(&mut self) -> io::Result<()> {
        self.command
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        match self.pipes.take() {
            Some(pipes) => pipes.finish(),
            None => Ok(()),
        }
    }
}

impl Drop for RunningOutput<'_> {
    fn drop(&mut self) {
        if let Err(source) = self.finish() {
            eprintln!("xtask: the owned command streams could not finish: {source}");
            std::process::abort();
        }
    }
}

fn finished(ran: Result<Ended, WorkError>, observed: io::Result<()>) -> Result<Ended, WorkError> {
    match (ran, observed) {
        (Ok(ended), Ok(())) => Ok(ended),
        (Err(failure), Ok(())) => Err(failure),
        (Ok(_ended), Err(source)) => Err(WorkError::Watch { source }),
        (Err(failure), Err(source)) => Err(WorkError::Watch {
            source: io::Error::other(format!(
                "{failure}; output or receipt also failed: {source}"
            )),
        }),
    }
}

#[derive(Debug)]
struct Pipes {
    stdout: Stream,
    stderr: Stream,
}

impl Pipes {
    fn launch(command: &mut Command, output: Output, events: &WorkEvents) -> io::Result<Self> {
        let (stdout, stderr) = match output {
            Output::Inherited => (
                Destination::Log(inherited_stdout()?),
                Destination::Log(inherited_stderr()?),
            ),
            Output::Log(log) => (Destination::Log(log.try_clone()?), Destination::Log(log)),
            Output::Capture { stdout, stderr } => {
                (Destination::Log(stdout), Destination::Log(stderr))
            }
        };
        let mut stdout = Stream::launch(stdout, events.clone())?;
        let mut stderr = Stream::launch(stderr, events.clone())?;
        let stdout_writer = stdout.writer()?;
        let stderr_writer = stderr.writer()?;
        command.stdout(stdout_writer).stderr(stderr_writer);
        Ok(Self { stdout, stderr })
    }

    fn finish(mut self) -> io::Result<()> {
        let stdout = self.stdout.finish();
        let stderr = self.stderr.finish();
        match (stdout, stderr) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(source), Ok(())) | (Ok(()), Err(source)) => Err(source),
            (Err(stdout), Err(stderr)) => Err(io::Error::other(format!(
                "stdout reader failed: {stdout}; stderr reader failed: {stderr}"
            ))),
        }
    }
}

#[derive(Debug)]
enum Destination {
    Log(std::fs::File),
}

impl Destination {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Log(log) => log.write_all(bytes),
        }
    }
}

#[cfg(unix)]
fn inherited_stdout() -> io::Result<std::fs::File> {
    use std::os::fd::AsFd as _;
    io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .map(std::fs::File::from)
}

#[cfg(unix)]
fn inherited_stderr() -> io::Result<std::fs::File> {
    use std::os::fd::AsFd as _;
    io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .map(std::fs::File::from)
}

#[cfg(windows)]
fn inherited_stdout() -> io::Result<std::fs::File> {
    use std::os::windows::io::AsHandle as _;
    io::stdout()
        .as_handle()
        .try_clone_to_owned()
        .map(std::fs::File::from)
}

#[cfg(windows)]
fn inherited_stderr() -> io::Result<std::fs::File> {
    use std::os::windows::io::AsHandle as _;
    io::stderr()
        .as_handle()
        .try_clone_to_owned()
        .map(std::fs::File::from)
}

#[derive(Debug)]
struct Stream {
    writer: Option<io::PipeWriter>,
    reader: Option<JoinHandle<io::Result<()>>>,
    #[cfg(unix)]
    endpoint: Option<njutest_process::OutputEndpoint>,
}

impl Stream {
    fn launch(mut destination: Destination, events: WorkEvents) -> io::Result<Self> {
        let (mut reading, writer) = io::pipe()?;
        #[cfg(unix)]
        let endpoint = njutest_process::OutputEndpoint::capture(&reading)?;
        let reader = std::thread::Builder::new()
            .name("xtask-output-arrivals".to_owned())
            .spawn(move || {
                let read = pump(&mut reading, &mut destination, &events);
                if let Err(source) = &read {
                    events.failed(io::Error::new(source.kind(), source.to_string()));
                }
                read
            })?;
        Ok(Self {
            writer: Some(writer),
            reader: Some(reader),
            #[cfg(unix)]
            endpoint: Some(endpoint),
        })
    }

    fn writer(&mut self) -> io::Result<io::PipeWriter> {
        self.writer
            .take()
            .ok_or_else(|| io::Error::other("the output writer was already handed to its command"))
    }

    fn finish(&mut self) -> io::Result<()> {
        let writer = self.writer.take();
        drop(writer);
        match self.reader.take() {
            Some(reader) => match reader.join() {
                Ok(observed) => observed,
                Err(panic) => {
                    drop(panic);
                    Err(io::Error::other("the owned output reader panicked"))
                }
            },
            None => Ok(()),
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Err(source) = self.finish() {
            eprintln!("xtask: an owned output reader could not finish: {source}");
            std::process::abort();
        }
    }
}

fn pump(
    reading: &mut io::PipeReader,
    destination: &mut Destination,
    events: &WorkEvents,
) -> io::Result<()> {
    let mut bytes = [0; 8192];
    loop {
        let read = reading.read(&mut bytes)?;
        if read == 0 {
            return Ok(());
        }
        let arrived = bytes
            .get(..read)
            .ok_or_else(|| io::Error::other("the pipe reader exceeded its actual buffer"))?;
        destination.write(arrived)?;
        events.heard();
    }
}

/// Captures a real command's original streams through the same owned completion and observation boundary.
pub(super) fn capture(
    command: &mut Command,
    environment: &Environment,
) -> io::Result<std::process::Output> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let stops = Stops::arm().map_err(io::Error::other)?;
    let ended = run(
        Request {
            command,
            bound: None,
            stops: &stops,
            environment,
            output: Output::Capture {
                stdout: stdout.try_clone()?,
                stderr: stderr.try_clone()?,
            },
        },
        |_leader| Ok(()),
    )
    .map_err(io::Error::other)?;
    let status = match ended {
        Ended::Exited(status) => status,
        Ended::Interrupted { .. } | Ended::OverBudget { .. } | Ended::Quiet { .. } => {
            return Err(io::Error::other(format!(
                "the actual probe was stopped: {ended:?}"
            )));
        }
    };
    stdout.rewind()?;
    stderr.rewind()?;
    let mut out = Vec::new();
    let mut err = Vec::new();
    io::BufReader::new(stdout).read_to_end(&mut out)?;
    io::BufReader::new(stderr).read_to_end(&mut err)?;
    Ok(std::process::Output {
        status,
        stdout: out,
        stderr: err,
    })
}
