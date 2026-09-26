// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The proxy a test dials instead of the thing it is talking to, which records what went past.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::rule::Rule;
use super::{Exchange, Spoken, Wire};
use crate::error;
#[cfg(feature = "testkit")]
use crate::error::ErrorCode;

/// How much of one exchange is read into memory before it is passed on.
const CHUNK: usize = 16 * 1024;

/// Where an interposer sits, and what it sits in front of.
#[derive(Debug, Clone)]
pub struct Interposing {
    /// The capability the seam serves, as `[resources.<name>]` names it.
    pub capability: String,
    /// What the test would have dialled.
    pub upstream: SocketAddr,
    /// How much of what goes past is read.
    pub wire: Wire,
    /// The one fault this run is measuring, when it is measuring one.
    /// `None` records and changes nothing.
    pub injecting: Option<super::derive::Fault>,
    /// How long an answer the run holds up is held for.
    pub held_up: std::time::Duration,
}

/// Why an interposer could not be put in front of a seam.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InterposeError {
    /// No port could be listened on.
    #[error("{}: interpose: cannot listen in front of {upstream}: {source}", error::WIRE_CANNOT_LISTEN.code)]
    CannotListen {
        /// What it would have sat in front of.
        upstream: SocketAddr,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
}

impl InterposeError {
    /// The stable code of this failure.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::CannotListen { .. } => error::WIRE_CANNOT_LISTEN,
        }
    }
}

/// An interposer that is listening, and what has gone past it.
#[derive(Debug)]
pub struct Interposer {
    address: SocketAddr,
    recorded: Arc<Mutex<Vec<Exchange>>>,
    sealed: Arc<AtomicBool>,
    carrying: Arc<AtomicBool>,
    running: Arc<Mutex<Option<String>>>,
    putting: Arc<Mutex<Option<super::derive::Fault>>>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    applied: Arc<AtomicBool>,
    previous: Arc<Mutex<Option<Vec<u8>>>>,
    incomplete: Arc<std::sync::atomic::AtomicU64>,
    serving: ServingThread,
}

impl Interposer {
    /// Listens in front of `interposing.upstream` and records every exchange that goes past.
    ///
    /// # Errors
    /// [`InterposeError::CannotListen`] when no port can be had.
    pub fn start(interposing: &Interposing) -> Result<Self, InterposeError> {
        let listener = bound_after_waiting_out_a_busy_machine().map_err(|source| {
            InterposeError::CannotListen {
                upstream: interposing.upstream,
                source,
            }
        })?;
        let address = listener
            .local_addr()
            .map_err(|source| InterposeError::CannotListen {
                upstream: interposing.upstream,
                source,
            })?;
        listener
            .set_nonblocking(true)
            .map_err(|source| InterposeError::CannotListen {
                upstream: interposing.upstream,
                source,
            })?;
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let sealed = Arc::new(AtomicBool::new(false));
        let carrying = Arc::new(AtomicBool::new(false));
        let running = Arc::new(Mutex::new(None));
        let putting = Arc::new(Mutex::new(interposing.injecting.clone()));
        let seq = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let applied = Arc::new(AtomicBool::new(false));
        let incomplete = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let previous = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(false));
        let serving = ServingThread::launch(
            listener,
            Serving {
                interposing: interposing.clone(),
                recorded: Arc::clone(&recorded),
                sealed: Arc::clone(&sealed),
                carrying: Arc::clone(&carrying),
                running: Arc::clone(&running),
                putting: Arc::clone(&putting),
                seq: Arc::clone(&seq),
                applied: Arc::clone(&applied),
                incomplete: Arc::clone(&incomplete),
                previous: Arc::clone(&previous),
                stopping: Arc::clone(&stopping),
            },
            stopping,
        );
        Ok(Self {
            address,
            recorded,
            sealed,
            carrying,
            running,
            putting,
            seq,
            applied,
            previous,
            incomplete,
            serving,
        })
    }

    /// Says who the run has running, so every exchange from here is stamped with it.
    ///
    /// `None` is the honest answer wherever the run cannot tell — several targets at once — and is better than the last name it happened to know,
    /// which would route a fault to tests that were not there.
    pub fn during(&self, who: Option<String>) {
        *locked(&self.running) = who;
    }

    /// Says which fault the seam is to put from here, and starts counting exchanges again.
    ///
    /// A fault names one exchange by its place in the order, so measuring a second fault means the suite is run again from the beginning and the count starts again with it.
    /// Carrying the old count over would name an exchange no run will reach, and every fault after the first would go past untouched while the report said it had been put.
    ///
    /// Waits like the observers do, and for a reason they share: an exchange still being carried is about to write the very count this resets, so resetting underneath it puts the count back where it was and the next fault names an exchange nothing reaches.
    pub fn putting(&self, fault: Option<super::derive::Fault>) {
        self.settled();
        *locked(&self.putting) = fault;
        self.seq.store(0, Ordering::Relaxed);
        self.applied.store(false, Ordering::Relaxed);
        *locked(&self.previous) = None;
    }

    /// Whether the exchange the fault names actually came past, so the question was really put.
    ///
    /// A suite that took a different path this time never reached the exchange the fault names, and every test still passed.
    /// Reading that as nothing noticing would report a gap the tests could close where the run never asked them anything at all.
    #[must_use]
    pub fn was_put(&self) -> bool {
        self.settled();
        self.applied.load(Ordering::Relaxed)
    }

    /// How many callers reached this seam and did not complete an exchange while no fault was in place.
    ///
    /// The interposer is the only thing that can tell those apart: a fault it applied is a question somebody asked, and an exchange that ended with nothing applied is the transport failing under whatever else the machine was doing.
    /// Reading the second as a test failing makes a busy runner into a verdict.
    #[must_use]
    pub fn did_not_complete(&self) -> u64 {
        self.incomplete.load(Ordering::Relaxed)
    }

    /// Hands back everything that has gone past so far and forgets it, without stopping.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn taken(&self) -> Vec<Exchange> {
        self.settled();
        locked(&self.recorded).drain(..).collect()
    }

    /// How long a boundary waits for an exchange already being carried to finish.
    ///
    /// Long enough for the last few instructions of a round trip under a profiler, and short enough that waiting the whole of it and carrying on is not something anybody would mistake for a hang.
    const SETTLING: std::time::Duration = std::time::Duration::from_millis(500);

    /// How long the waiting thread stands aside for each time round.
    ///
    /// Sleeping rather than yielding, because the thread being waited for is the one that has to run for the wait to end.
    /// On a machine with more runnable threads than cores — a profiler's, a hosted runner's — a yielding loop is handed straight back its own core and starves the thread it is waiting for, which is how a wait bounded at half a second manages to time out.
    const BREATH: std::time::Duration = std::time::Duration::from_micros(200);

    /// Waits for the exchange being carried, if there is one, before anything reads or resets what it is about to write.
    ///
    /// A caller's request returns when the interposer closes the connection to it, and everything else happens after that: the exchange is written down, the count moves on, the fault is marked as put.
    /// So anything done the instant the last caller was answered meets a state the run is still leaving — a recording cleared before the exchange before it landed, a fault reported as never put when it was put half a microsecond ago, or a count reset that the exchange in the air then puts back.
    ///
    /// The rule is one sentence: a question about what an interposer has seen is only answerable once it has finished seeing it, and resetting what it is about to write is the same question asked backwards.
    /// Serving one connection at a time is what makes this a single flag rather than a count.
    fn settled(&self) {
        let since = std::time::Instant::now();
        while self.carrying.load(Ordering::Acquire) && since.elapsed() < Self::SETTLING {
            std::thread::sleep(Self::BREATH);
        }
    }

    /// Forgets everything recorded and starts recording again, from exchange zero.
    ///
    /// Everything `putting` resets is reset here for the same reason: a catalogue names an exchange by its place in the order, so a recording that carried a count over from an earlier run of the suite would name exchanges no single run reaches.
    pub fn restart(&self) {
        self.settled();
        self.sealed.store(false, Ordering::Relaxed);
        locked(&self.recorded).clear();
        self.putting(None);
    }

    /// Stops recording and hands back what went past up to this point, keeping it.
    ///
    /// Everything downstream of a recording derives a catalogue from it, and a catalogue is a set of questions about the program the tests are about.
    /// Traffic from a run of a mutated program, or from a run with a fault already in place, is traffic from a different program, and a question derived from it is a question about a program nobody asked after.
    /// The interposer keeps carrying, counting and applying after this; it only stops adding to what a catalogue can be made of.
    #[must_use]
    pub fn seal(&self) -> Vec<Exchange> {
        self.settled();
        self.sealed.store(true, Ordering::Relaxed);
        locked(&self.recorded).clone()
    }

    /// Where a test is told to dial.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Stops listening and hands back everything that went past, in the order it did.
    #[must_use]
    pub fn stop(mut self) -> Vec<Exchange> {
        if self.serving.stop().is_err() {
            std::process::abort();
        }
        locked(&self.recorded).clone()
    }
}

/// The listening thread and the stop capability that always joins it.
#[derive(Debug)]
struct ServingThread {
    stopping: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl ServingThread {
    fn launch(listener: TcpListener, serving: Serving, stopping: Arc<AtomicBool>) -> Self {
        let handle = std::thread::spawn(move || serve(&listener, &serving));
        Self {
            stopping,
            handle: Some(handle),
        }
    }

    fn stop(&mut self) -> Result<(), ServingError> {
        self.stopping.store(true, Ordering::Release);
        self.join()
    }

    fn join(&mut self) -> Result<(), ServingError> {
        let Some(handle) = self.handle.take() else {
            return Err(ServingError::AlreadyStopped);
        };
        match handle.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) => Err(ServingError::Accept { source }),
            Err(panic) => {
                drop(panic);
                Err(ServingError::Panicked)
            }
        }
    }
}

impl Drop for ServingThread {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if self.handle.is_some() && self.join().is_err() {
            std::process::abort();
        }
    }
}

/// Why the listening thread could not finish as one complete recording.
#[derive(Debug, thiserror::Error)]
enum ServingError {
    /// A terminal operation had already consumed the handle.
    #[error("the interposer serving thread had already stopped")]
    AlreadyStopped,
    /// The listening thread could no longer accept connections.
    #[error("the interposer serving thread could not accept a connection: {source}")]
    Accept {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// The listening thread panicked before completing the recording.
    #[error("the interposer serving thread panicked")]
    Panicked,
}

/// What the listening thread was given, as one value.
struct Serving {
    interposing: Interposing,
    recorded: Arc<Mutex<Vec<Exchange>>>,
    sealed: Arc<AtomicBool>,
    carrying: Arc<AtomicBool>,
    running: Arc<Mutex<Option<String>>>,
    putting: Arc<Mutex<Option<super::derive::Fault>>>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    applied: Arc<AtomicBool>,
    previous: Arc<Mutex<Option<Vec<u8>>>>,
    stopping: Arc<AtomicBool>,
    incomplete: Arc<std::sync::atomic::AtomicU64>,
}

/// Accepts one connection at a time until asked to stop.
fn serve(listener: &TcpListener, serving: &Serving) -> std::io::Result<()> {
    while !serving.stopping.load(Ordering::Relaxed) {
        let downstream = match listener.accept() {
            Ok((downstream, _from)) => downstream,
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            Err(source) => return Err(source),
        };
        downstream.set_nonblocking(false)?;
        if serving.stopping.load(Ordering::Relaxed) {
            return Ok(());
        }
        serving.carrying.store(true, Ordering::Release);
        let seq = serving.seq.load(Ordering::Relaxed);
        let during = locked(&serving.running).clone();
        let putting = locked(&serving.putting).clone();
        let previous = locked(&serving.previous).clone();
        let carried = carry(
            downstream,
            &serving.interposing,
            Carrying {
                seq,
                during,
                putting,
                previous,
            },
        );
        if carried.applied {
            serving.applied.store(true, Ordering::Relaxed);
            *locked(&serving.putting) = None;
        }
        if carried.exchange.is_none() && !carried.applied {
            match serving
                .incomplete
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |seen| {
                    seen.checked_add(1)
                }) {
                Ok(_seen) => {}
                Err(_more_callers_than_can_be_counted) => std::process::abort(),
            }
        }
        if let Some(exchange) = carried.exchange {
            if !serving.sealed.load(Ordering::Relaxed) {
                locked(&serving.recorded).push(exchange);
            }
            *locked(&serving.previous) = carried.upstream_said;
            let Some(next) = seq.checked_add(1) else {
                std::process::abort();
            };
            serving.seq.store(next, Ordering::Relaxed);
        }
        serving.carrying.store(false, Ordering::Release);
    }
    Ok(())
}

/// Takes the explicit recovery policy for every interposer lock: a panic in a connection must not make all later state updates disappear.
/// The value is still structurally valid, so the next owner continues from the poisoned guard rather than pretending the update succeeded without writing it.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(held) => held,
        Err(poisoned) => {
            drop(poisoned);
            std::process::abort();
        }
    }
}

/// What one exchange is carried with: where it falls in the order, who asked for it, and what is being put.
struct Carrying {
    seq: u64,
    during: Option<String>,
    putting: Option<super::derive::Fault>,
    previous: Option<Vec<u8>>,
}

/// What carrying one exchange came to.
#[derive(Debug, Default)]
struct Carried {
    /// What went past, or nothing where nothing did.
    exchange: Option<Exchange>,
    /// Whether the question the run is putting was really put here.
    applied: bool,
    /// What the upstream said before anything was done to it, which the next exchange may be answered with.
    upstream_said: Option<Vec<u8>>,
}

/// Carries one exchange to the upstream and back, and says what it was.
fn carry(mut downstream: TcpStream, interposing: &Interposing, carrying: Carrying) -> Carried {
    let Carrying {
        seq,
        during,
        putting,
        previous,
    } = carrying;
    let started = std::time::Instant::now();
    let Some(spoke) = exchanged(&mut downstream, interposing) else {
        return Carried::default();
    };
    let (asked, said) = spoke;
    let put = names(
        putting.as_ref(),
        &interposing.capability,
        seq,
        Rule::ReplayRequest,
    ) && delivered_again(interposing.upstream, &asked);
    let done = injected(
        (&interposing.capability, interposing.held_up),
        putting.as_ref(),
        (seq, previous),
        said.clone(),
    );
    let applied = put || done.applied;
    let Some(answered) = done.answered else {
        return Carried {
            exchange: None,
            applied,
            upstream_said: None,
        };
    };
    if downstream.write_all(&answered).is_err() {
        return Carried {
            exchange: None,
            applied,
            upstream_said: None,
        };
    }
    if downstream.flush().is_err() {
        return Carried {
            exchange: None,
            applied,
            upstream_said: None,
        };
    }
    let duration_ms = exact_duration_millis(started.elapsed());
    Carried {
        exchange: Some(Exchange {
            capability: interposing.capability.clone(),
            seq,
            during,
            duration_ms,
            spoken: spoken(interposing.wire, &asked, &answered),
        }),
        applied,
        upstream_said: Some(said),
    }
}

/// What the caller asked and what the upstream said, before anything is done to either.
fn exchanged(downstream: &mut TcpStream, interposing: &Interposing) -> Option<(Vec<u8>, Vec<u8>)> {
    let asked = first(downstream)?;
    let mut upstream = match TcpStream::connect(interposing.upstream) {
        Ok(upstream) => upstream,
        Err(_) => return None,
    };
    if upstream.write_all(&asked).is_err() || upstream.flush().is_err() {
        return None;
    }
    let mut said = Vec::new();
    if upstream.read_to_end(&mut said).is_err() {
        return None;
    }
    Some((asked, said))
}

/// Whether `putting` is the question of `rule` about this exchange of this seam.
fn names(putting: Option<&super::derive::Fault>, capability: &str, seq: u64, rule: Rule) -> bool {
    putting.is_some_and(|fault| {
        fault.capability == capability && fault.seq == seq && fault.rule == rule
    })
}

/// Sends the request upstream a second time and reads what it said, which nobody is told.
///
/// The caller is handed the first answer, exactly as a retry after a lost answer leaves it: the dependency did the work twice and the caller never knew.
/// What notices is whatever holds the dependency's state, and a suite that notices nothing is a suite that would not notice a double charge.
fn delivered_again(upstream: SocketAddr, asked: &[u8]) -> bool {
    let Ok(mut again) = TcpStream::connect(upstream) else {
        return false;
    };
    if again.write_all(asked).is_err() {
        return false;
    }
    if again.flush().is_err() {
        return false;
    }
    let mut ignored = Vec::new();
    again.read_to_end(&mut ignored).is_ok()
}

/// How long a run holds an answer up for where nothing else says.
///
/// Long enough that a suite which waits on the seam without a deadline of its own is noticed for waiting, and short enough that a run measuring one is not mistaken for a run that hung.
pub const HELD_UP: std::time::Duration = std::time::Duration::from_secs(30);

/// What the caller is given, once the fault this run measures has had its say.
///
/// A fault names one exchange on one seam, so an exchange it does not name goes past untouched: changing a second one would measure two faults and report one.
/// `None` says the caller is given nothing at all.
fn injected(
    (capability, held_up): (&str, std::time::Duration),
    putting: Option<&super::derive::Fault>,
    (seq, previous): (u64, Option<Vec<u8>>),
    answered: Vec<u8>,
) -> Done {
    let Some(fault) = putting else {
        return Done::untouched(answered);
    };
    if fault.capability != capability || fault.seq != seq {
        return Done::untouched(answered);
    }
    match fault.rule {
        Rule::DropConnection => Done {
            answered: None,
            applied: true,
        },
        Rule::TruncateResponse => Done::put(cut(answered)),
        Rule::DelayResponse => {
            std::thread::sleep(held_up);
            Done::put(answered)
        }
        Rule::StatusServerError | Rule::StatusNotFound => fault.rule.restates().map_or_else(
            || Done::untouched(answered.clone()),
            |(status, reason)| Done::put(restated(&answered, status, reason)),
        ),
        Rule::StaleResponse => previous.map_or_else(|| Done::untouched(answered), Done::put),
        Rule::ReplayRequest => Done {
            answered: Some(answered),
            applied: true,
        },
    }
}

/// What the caller is handed, and whether the question was really put.
///
/// A rule nothing here carries out leaves `applied` false rather than passing the answer along quietly.
/// A question the interposer cannot put is one the run established nothing about, and calling it a survivor because the tests then passed would report a gap no test could ever close.
#[derive(Debug)]
struct Done {
    answered: Option<Vec<u8>>,
    applied: bool,
}

impl Done {
    /// The answer as the upstream gave it, with nothing put.
    const fn untouched(answered: Vec<u8>) -> Self {
        Self {
            answered: Some(answered),
            applied: false,
        }
    }

    /// The answer the question asks for.
    const fn put(answered: Vec<u8>) -> Self {
        Self {
            answered: Some(answered),
            applied: true,
        }
    }
}

/// The answer with its body cut away, as a connection that died mid-body leaves it.
fn cut(answered: Vec<u8>) -> Vec<u8> {
    let head = head_position(&answered);
    answered.get(..head).map(<[u8]>::to_vec).unwrap_or(answered)
}

/// What `restated` writes in place of a status line, from the line alone.
///
/// The proof that restating an answer changes nothing compares what this would write against what the upstream wrote.
/// Sharing the function is what makes that a check rather than a second opinion: a proof about what the injection does, held to the injection.
#[must_use]
pub fn restated_line(line: &str, status: u16, reason: &str) -> String {
    let version = line.split(' ').next().unwrap_or_default();
    let version = if version.is_empty() {
        "HTTP/1.1"
    } else {
        version
    };
    format!("{version} {status} {reason}")
}

/// The answer with its status line restated, and everything else as it was.
fn restated(answered: &[u8], status: u16, reason: &str) -> Vec<u8> {
    let end = answered
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .unwrap_or(answered.len());
    let line = answered
        .get(..end)
        .and_then(|line| match std::str::from_utf8(line) {
            Ok(line) => Some(line),
            Err(_) => None,
        });
    let line = line.unwrap_or_default();
    let mut out = restated_line(line, status, reason).into_bytes();
    if let Some(rest) = answered.get(end..) {
        out.extend_from_slice(rest);
    }
    out
}

/// Everything the test sent before it stopped talking, up to one chunk.
fn first(downstream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buffer = vec![0_u8; CHUNK];
    let read = match downstream.read(&mut buffer) {
        Ok(read) => read,
        Err(_) => return None,
    };
    if read == 0 {
        return None;
    }
    buffer.truncate(read);
    Some(buffer)
}

/// What was said, read as far as the wire says to read it.
fn spoken(wire: Wire, asked: &[u8], answered: &[u8]) -> Spoken {
    let request_bytes = exact_byte_count(asked.len());
    let response_bytes = exact_byte_count(answered.len());
    match wire {
        Wire::Raw => Spoken::Raw {
            request_bytes,
            response_bytes,
        },
        Wire::Http => {
            let Some((method, path)) = requested(asked) else {
                return Spoken::Raw {
                    request_bytes,
                    response_bytes,
                };
            };
            Spoken::Http {
                method,
                path,
                status: status(answered).unwrap_or_default(),
                request_bytes,
                response_bytes,
                body_bytes: exact_body_bytes(response_bytes, head_of(answered)),
                status_line: opening(answered).unwrap_or_default(),
            }
        }
    }
}

/// How many bytes of an answer are its head, terminator and all.
///
/// An answer with no terminator in it is all head, which is what makes cutting it short a change of nothing rather than a guess at where the body began.
fn head_of(answered: &[u8]) -> u64 {
    exact_byte_count(head_position(answered))
}

fn head_position(answered: &[u8]) -> usize {
    let Some(start) = answered.windows(4).position(|four| four == b"\r\n\r\n") else {
        return answered.len();
    };
    match start.checked_add(4) {
        Some(end) => end,
        None => std::process::abort(),
    }
}

fn exact_body_bytes(response: u64, head: u64) -> u64 {
    match response.checked_sub(head) {
        Some(body) => body,
        None => std::process::abort(),
    }
}

fn exact_duration_millis(duration: std::time::Duration) -> u64 {
    match u64::try_from(duration.as_millis()) {
        Ok(milliseconds) => milliseconds,
        Err(_) => std::process::abort(),
    }
}

fn exact_byte_count(bytes: usize) -> u64 {
    match u64::try_from(bytes) {
        Ok(count) => count,
        Err(_) => std::process::abort(),
    }
}

/// The method and the path of an HTTP request, without the query.
fn requested(asked: &[u8]) -> Option<(String, String)> {
    let line = opening(asked)?;
    let mut words = line.split(' ');
    let method = words.next()?;
    let target = words.next()?;
    if method.is_empty() || target.is_empty() {
        return None;
    }
    let path = target.split('?').next().unwrap_or(target);
    Some((method.to_owned(), path.to_owned()))
}

/// The status an HTTP response opened with.
fn status(answered: &[u8]) -> Option<u16> {
    let line = opening(answered)?;
    match line.split(' ').nth(1)?.parse::<u16>() {
        Ok(status) => Some(status),
        Err(_) => None,
    }
}

/// The first line of what went past, as text.
fn opening(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .unwrap_or(bytes.len());
    let line = bytes.get(..end)?;
    match std::str::from_utf8(line) {
        Ok(line) => Some(line.to_owned()),
        Err(_) => None,
    }
}

/// How long a seam waits for an ephemeral port before deciding the machine has none to give.
///
/// A port comes free as connections leave `TIME_WAIT`, so this is long enough to outlast a burst and short enough that a machine genuinely out of ports still says so promptly.
const A_PORT_COMES_FREE_WITHIN: std::time::Duration = std::time::Duration::from_millis(500);

/// How often a seam asks again for a port while the machine has none.
const ASK_AGAIN_EVERY: std::time::Duration = std::time::Duration::from_millis(10);

/// Takes an ephemeral port, waiting out a machine that momentarily has none rather than giving the seam up.
///
/// A single attempt made a busy runner into an unwatched seam: the run carried on, derived its questions from the one seam left, and reported a block half the size with no row saying the other seam was never watched.
/// Asking once for something that is transiently scarce is what turned port pressure into a gap in an assurance report.
fn bound_after_waiting_out_a_busy_machine() -> std::io::Result<TcpListener> {
    let started = std::time::Instant::now();
    loop {
        match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => return Ok(listener),
            Err(error) if started.elapsed() < A_PORT_COMES_FREE_WITHIN => {
                drop(error);
                std::thread::sleep(ASK_AGAIN_EVERY);
            }
            Err(error) => return Err(error),
        }
    }
}
