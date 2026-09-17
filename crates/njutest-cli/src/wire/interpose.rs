// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The proxy a test dials instead of the thing it is talking to, which records what went past.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{Exchange, Spoken, Wire};
use crate::error::{self, ErrorCode};

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
    /// The one fault this run is measuring, when it is measuring one. `None` records and changes nothing.
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
    running: Arc<Mutex<Option<String>>>,
    putting: Arc<Mutex<Option<super::derive::Fault>>>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    applied: Arc<AtomicBool>,
    previous: Arc<Mutex<Option<Vec<u8>>>>,
    stopping: Arc<AtomicBool>,
    serving: Option<std::thread::JoinHandle<()>>,
}

impl Interposer {
    /// Listens in front of `interposing.upstream` and records every exchange that goes past.
    ///
    /// # Errors
    /// [`InterposeError::CannotListen`] when no port can be had.
    pub fn start(interposing: &Interposing) -> Result<Self, InterposeError> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|source| InterposeError::CannotListen {
                upstream: interposing.upstream,
                source,
            })?;
        let address = listener
            .local_addr()
            .map_err(|source| InterposeError::CannotListen {
                upstream: interposing.upstream,
                source,
            })?;
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let running = Arc::new(Mutex::new(None));
        let putting = Arc::new(Mutex::new(interposing.injecting.clone()));
        let seq = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let applied = Arc::new(AtomicBool::new(false));
        let previous = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(false));
        let serving = std::thread::spawn({
            let serving = Serving {
                interposing: interposing.clone(),
                recorded: Arc::clone(&recorded),
                running: Arc::clone(&running),
                putting: Arc::clone(&putting),
                seq: Arc::clone(&seq),
                applied: Arc::clone(&applied),
                previous: Arc::clone(&previous),
                stopping: Arc::clone(&stopping),
            };
            move || serve(&listener, &serving)
        });
        Ok(Self {
            address,
            recorded,
            running,
            putting,
            seq,
            applied,
            previous,
            stopping,
            serving: Some(serving),
        })
    }

    /// Says who the run has running, so every exchange from here is stamped with it.
    ///
    /// `None` is the honest answer wherever the run cannot tell — several
    /// targets at once — and is better than the last name it happened to know,
    /// which would route a fault to tests that were not there.
    pub fn during(&self, who: Option<String>) {
        if let Ok(mut held) = self.running.lock() {
            *held = who;
        }
    }

    /// Says which fault the seam is to put from here, and starts counting exchanges again.
    ///
    /// A fault names one exchange by its place in the order, so measuring a
    /// second fault means the suite is run again from the beginning and the
    /// count starts again with it. Carrying the old count over would name an
    /// exchange no run will reach, and every fault after the first would go
    /// past untouched while the report said it had been put.
    pub fn putting(&self, fault: Option<super::derive::Fault>) {
        if let Ok(mut held) = self.putting.lock() {
            *held = fault;
        }
        self.seq.store(0, Ordering::Relaxed);
        self.applied.store(false, Ordering::Relaxed);
        if let Ok(mut held) = self.previous.lock() {
            *held = None;
        }
    }

    /// Whether the exchange the fault names actually came past, so the question was really put.
    ///
    /// A suite that took a different path this time never reached the exchange
    /// the fault names, and every test still passed. Reading that as nothing
    /// noticing would report a gap the tests could close where the run never
    /// asked them anything at all.
    #[must_use]
    pub fn was_put(&self) -> bool {
        self.applied.load(Ordering::Relaxed)
    }

    /// Hands back everything that has gone past so far and forgets it, without stopping.
    #[must_use]
    pub fn taken(&self) -> Vec<Exchange> {
        self.recorded
            .lock()
            .map(|mut held| std::mem::take(&mut *held))
            .unwrap_or_default()
    }

    /// Where a test is told to dial.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Stops listening and hands back everything that went past, in the order it did.
    #[must_use]
    pub fn stop(mut self) -> Vec<Exchange> {
        self.stopping.store(true, Ordering::Relaxed);
        let _woken = TcpStream::connect(self.address);
        if let Some(serving) = self.serving.take() {
            let _joined = serving.join();
        }
        self.recorded
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }
}

/// What the listening thread was given, as one value.
struct Serving {
    interposing: Interposing,
    recorded: Arc<Mutex<Vec<Exchange>>>,
    running: Arc<Mutex<Option<String>>>,
    putting: Arc<Mutex<Option<super::derive::Fault>>>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    applied: Arc<AtomicBool>,
    previous: Arc<Mutex<Option<Vec<u8>>>>,
    stopping: Arc<AtomicBool>,
}

/// Accepts one connection at a time until asked to stop.
fn serve(listener: &TcpListener, serving: &Serving) {
    while !serving.stopping.load(Ordering::Relaxed) {
        let Ok((downstream, _from)) = listener.accept() else {
            return;
        };
        if serving.stopping.load(Ordering::Relaxed) {
            return;
        }
        let seq = serving.seq.load(Ordering::Relaxed);
        let during = serving.running.lock().ok().and_then(|held| held.clone());
        let putting = serving.putting.lock().ok().and_then(|held| held.clone());
        let previous = serving.previous.lock().ok().and_then(|held| held.clone());
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
        }
        if let Some(exchange) = carried.exchange {
            if let Ok(mut held) = serving.recorded.lock() {
                held.push(exchange);
            }
            if let Ok(mut held) = serving.previous.lock() {
                *held = carried.upstream_said;
            }
            serving.seq.store(seq.saturating_add(1), Ordering::Relaxed);
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
        "replay-request",
    );
    if put {
        delivered_again(interposing.upstream, &asked);
    }
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
    let _flushed = downstream.flush();
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
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
    let mut upstream = TcpStream::connect(interposing.upstream).ok()?;
    upstream.write_all(&asked).ok()?;
    upstream.flush().ok()?;
    let mut said = Vec::new();
    let _read = upstream.read_to_end(&mut said).ok()?;
    Some((asked, said))
}

/// Whether `putting` is the question of `rule` about this exchange of this seam.
fn names(putting: Option<&super::derive::Fault>, capability: &str, seq: u64, rule: &str) -> bool {
    putting.is_some_and(|fault| {
        fault.capability == capability && fault.seq == seq && fault.rule == rule
    })
}

/// Sends the request upstream a second time and reads what it said, which nobody is told.
///
/// The caller is handed the first answer, exactly as a retry after a lost
/// answer leaves it: the dependency did the work twice and the caller never
/// knew. What notices is whatever holds the dependency's state, and a suite
/// that notices nothing is a suite that would not notice a double charge.
fn delivered_again(upstream: SocketAddr, asked: &[u8]) {
    let Ok(mut again) = TcpStream::connect(upstream) else {
        return;
    };
    if again.write_all(asked).is_err() {
        return;
    }
    let _flushed = again.flush();
    let mut ignored = Vec::new();
    let _read = again.read_to_end(&mut ignored);
}

/// How long a run holds an answer up for where nothing else says.
///
/// Long enough that a suite which waits on the seam without a deadline of its
/// own is noticed for waiting, and short enough that a run measuring one is
/// not mistaken for a run that hung.
pub const HELD_UP: std::time::Duration = std::time::Duration::from_secs(30);

/// What the caller is given, once the fault this run measures has had its say.
///
/// A fault names one exchange on one seam, so an exchange it does not name
/// goes past untouched: changing a second one would measure two faults and
/// report one. `None` says the caller is given nothing at all.
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
    match fault.rule.as_str() {
        "drop-connection" => Done {
            answered: None,
            applied: true,
        },
        "truncate-response" => Done::put(cut(answered)),
        "delay-response" => {
            std::thread::sleep(held_up);
            Done::put(answered)
        }
        "status-server-error" => Done::put(restated(&answered, 500, "Internal Server Error")),
        "status-not-found" => Done::put(restated(&answered, 404, "Not Found")),
        "stale-response" => previous.map_or_else(|| Done::untouched(answered), Done::put),
        _ => Done::untouched(answered),
    }
}

/// What the caller is handed, and whether the question was really put.
///
/// A rule nothing here carries out leaves `applied` false rather than passing
/// the answer along quietly. A question the interposer cannot put is one the
/// run established nothing about, and calling it a survivor because the tests
/// then passed would report a gap no test could ever close.
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
    let head = answered
        .windows(4)
        .position(|four| four == b"\r\n\r\n")
        .map_or(answered.len(), |at| at.saturating_add(4));
    answered.get(..head).map(<[u8]>::to_vec).unwrap_or(answered)
}

/// The answer with its status line restated, and everything else as it was.
fn restated(answered: &[u8], status: u16, reason: &str) -> Vec<u8> {
    let end = answered
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .unwrap_or(answered.len());
    let version = answered
        .get(..end)
        .and_then(|line| std::str::from_utf8(line).ok())
        .and_then(|line| line.split(' ').next().map(ToOwned::to_owned))
        .unwrap_or_else(|| "HTTP/1.1".to_owned());
    let mut out = format!("{version} {status} {reason}").into_bytes();
    if let Some(rest) = answered.get(end..) {
        out.extend_from_slice(rest);
    }
    out
}

/// Everything the test sent before it stopped talking, up to one chunk.
fn first(downstream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buffer = vec![0_u8; CHUNK];
    let read = downstream.read(&mut buffer).ok()?;
    if read == 0 {
        return None;
    }
    buffer.truncate(read);
    Some(buffer)
}

/// What was said, read as far as the wire says to read it.
fn spoken(wire: Wire, asked: &[u8], answered: &[u8]) -> Spoken {
    let request_bytes = u64::try_from(asked.len()).unwrap_or(u64::MAX);
    let response_bytes = u64::try_from(answered.len()).unwrap_or(u64::MAX);
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
                body_bytes: response_bytes.saturating_sub(head_of(answered)),
            }
        }
    }
}

/// How many bytes of an answer are its head, terminator and all.
///
/// An answer with no terminator in it is all head, which is what makes cutting
/// it short a change of nothing rather than a guess at where the body began.
fn head_of(answered: &[u8]) -> u64 {
    let head = answered
        .windows(4)
        .position(|four| four == b"\r\n\r\n")
        .map_or(answered.len(), |at| at.saturating_add(4));
    u64::try_from(head).unwrap_or(u64::MAX)
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
    line.split(' ').nth(1)?.parse().ok()
}

/// The first line of what went past, as text.
fn opening(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .unwrap_or(bytes.len());
    let line = bytes.get(..end)?;
    std::str::from_utf8(line).ok().map(ToOwned::to_owned)
}
