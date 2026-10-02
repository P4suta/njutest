// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Actual output arrivals and measured receipts for the existing owned command runner.

use std::io::{self, Read as _, Seek as _, Write as _};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Instant;

use super::{Output, Request};
use crate::environment::Environment;
use crate::work::{self, Ended, Stops, WorkError, WorkEvents};

/// Captures actual bytes before notifying the work observer and publishes every result's waits.
pub(super) fn run<F>(request: Request<'_, '_>, started: F) -> Result<Ended, WorkError>
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
    let began = Instant::now();
    let invocation = super::hostcost::Invocation::of(command);
    let events = stops.events();
    let leader = std::cell::Cell::new(None);
    let (ran, joined) = match RunningOutput::launch(command, output, &events) {
        Ok(mut reading) => {
            let ran = work::run(reading.command, bound, stops, |pid| {
                leader.set(Some(pid));
                started(pid)
            });
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
}

impl Stream {
    fn launch(mut destination: Destination, events: WorkEvents) -> io::Result<Self> {
        let (mut reading, writer) = io::pipe()?;
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
