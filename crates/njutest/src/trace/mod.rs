// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runner's trace: what a run did while it did it, as diagnostic exhaust.

mod event;
mod reader;
mod sink;

#[cfg(feature = "testkit")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "testkit")]
use std::time::Duration;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

#[cfg(feature = "testkit")]
pub use event::SCHEMA;
pub use event::{
    ArtifactRecord, AskedRecord, DischargeRecord, DriftRecord, Event, ExecRecord, MutantExecRecord,
    NoteRecord, Payload, PhaseRecord, ProbeExecRecord, ProgressRecord, Read, RepairRecord,
    RouteRecord, RunAccounting, RunRecord, SentinelRecord, SiteReached, StartRecord,
    WireExchangeRecord, WireExecRecord,
};
pub use reader::{Problem, ReadError, check, read_events};
pub use sink::{DirSink, FILE_NAME, OUTPUT_DIRECTORY_NAME, Sink};
#[cfg(feature = "testkit")]
pub use sink::{MemorySink, RING_CAPACITY};

use crate::report::ConclusionAccounting;

/// A source of the current moment: the recorder's one test seam, so a test can freeze time and a golden can freeze the wire shape.
#[cfg(any(test, feature = "testkit"))]
#[derive(Debug)]
pub enum Clock {
    /// The moment it actually is.
    Wall,
    /// One that starts at `origin` and advances by `step` each reading, so a recording is the same bytes every time it is made.
    #[cfg(feature = "testkit")]
    Stepping {
        /// The first moment it answers with.
        origin: Timestamp,
        /// How far it moves per reading.
        step: Duration,
        /// How many readings there have been.
        readings: AtomicU64,
    },
}

/// Production has exactly one clock.
/// Keeping that state private means the deterministic injection seam does not become part of an installed CLI's library surface.
#[cfg(not(any(test, feature = "testkit")))]
#[derive(Debug)]
enum Clock {
    Wall,
}

#[derive(Debug, thiserror::Error)]
enum ClockError {
    #[error("the deterministic trace clock exhausted its u64 reading counter")]
    #[cfg(feature = "testkit")]
    ReadingsExhausted,
    #[cfg(feature = "testkit")]
    #[error("the deterministic trace clock reading does not fit its u32 step multiplier")]
    StepOutsideRange(#[source] std::num::TryFromIntError),
    #[cfg(feature = "testkit")]
    #[error("the deterministic trace clock duration overflowed")]
    DurationOverflow,
    #[cfg(feature = "testkit")]
    #[error("the deterministic trace clock left the timestamp range")]
    TimestampOutsideRange,
    #[error("the elapsed trace duration does not fit the wire's u64 milliseconds")]
    ElapsedOutsideWire(#[source] std::num::TryFromIntError),
}

impl Clock {
    const fn wall() -> Self {
        #[cfg(any(test, feature = "testkit"))]
        {
            Self::Wall
        }
        #[cfg(not(any(test, feature = "testkit")))]
        {
            Self::Wall
        }
    }

    #[cfg(any(test, feature = "testkit"))]
    /// A clock that starts at `origin` and advances by `step` per reading.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn stepping(origin: Timestamp, step: Duration) -> Self {
        Self::Stepping {
            origin,
            step,
            readings: AtomicU64::new(0),
        }
    }

    fn start(&self) -> Timestamp {
        #[cfg(not(any(test, feature = "testkit")))]
        match self {
            Self::Wall => Timestamp::now(),
        }
        #[cfg(any(test, feature = "testkit"))]
        match self {
            Self::Wall => Timestamp::now(),
            #[cfg(feature = "testkit")]
            Self::Stepping {
                origin, readings, ..
            } => {
                readings.store(1, Ordering::SeqCst);
                *origin
            }
        }
    }

    /// The next moment, or the exact representational failure that made the deterministic clock unusable.
    #[cfg(any(test, feature = "testkit"))]
    fn now(&self) -> Result<Timestamp, ClockError> {
        match self {
            Self::Wall => Ok(Timestamp::now()),
            #[cfg(feature = "testkit")]
            Self::Stepping {
                origin,
                step,
                readings,
            } => {
                let reading = readings
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                        current.checked_add(1)
                    })
                    .map_err(|_last_reading| ClockError::ReadingsExhausted)?;
                let steps = u32::try_from(reading).map_err(ClockError::StepOutsideRange)?;
                let elapsed = step
                    .checked_mul(steps)
                    .ok_or(ClockError::DurationOverflow)?;
                origin
                    .checked_add(elapsed)
                    .map_err(|_outside_range| ClockError::TimestampOutsideRange)
            }
        }
    }

    #[cfg(not(any(test, feature = "testkit")))]
    fn now(&self) -> Timestamp {
        match self {
            Self::Wall => Timestamp::now(),
        }
    }
}

/// Turns the events of a run into a stream a sink keeps.
#[derive(Clone)]
pub struct Recorder {
    inner: Option<Arc<Inner>>,
}

impl Default for Recorder {
    /// The trace that records nothing.
    fn default() -> Self {
        Self::disabled()
    }
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("enabled", &self.inner.is_some())
            .finish()
    }
}

struct Inner {
    sink: Sink,
    clock: Clock,
    started: Timestamp,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    seq: u64,
    attempts: u64,
    failures: u64,
    durable_failure: Option<std::io::Error>,
    accounting_failed: bool,
    ended: bool,
    /// The stage the run said it was in, and when it said so.
    stage: Option<(String, Timestamp)>,
}

enum EndDelivery {
    AlreadyEnded,
    Delivered(Option<std::io::Error>),
}

struct EndInput {
    moment: Timestamp,
    verdict: crate::report::Verdict,
    accounting: Option<ConclusionAccounting>,
    error: Option<String>,
}

impl Recorder {
    /// The trace that records nothing.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Starts a recording into `sink` with an injected test clock and emits its `run-start` event.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn new(sink: Sink, clock: Clock, start: StartRecord) -> Self {
        Self::from_clock(sink, clock, start)
    }

    fn from_clock(sink: Sink, clock: Clock, start: StartRecord) -> Self {
        let started = clock.start();
        let inner = Arc::new(Inner {
            sink,
            clock,
            started,
            state: Mutex::new(State::default()),
        });
        let recorder = Self { inner: Some(inner) };
        recorder.emit_at(started, Payload::RunStart { start });
        recorder
    }

    /// A recording on the wall clock, the test-injectable constructor's sibling.
    #[must_use]
    pub fn wall(sink: Sink, start: StartRecord) -> Self {
        Self::from_clock(sink, Clock::wall(), start)
    }

    /// Every event a memory sink of this recording kept, oldest first.
    #[cfg(feature = "testkit")]
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.inner
            .as_ref()
            .map(|inner| inner.sink.events())
            .unwrap_or_default()
    }

    /// Whether anything is recorded.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Injects a durable-write failure after recording has started.
    #[cfg(feature = "testkit")]
    pub fn fail_durable_writes_for_test(&self) {
        if let Some(inner) = &self.inner {
            inner.sink.fail_durable_writes();
        }
    }

    /// Records the beginning of a phase and returns the guard that ends it.
    /// The guard ends the phase once, on [`Phase::end`] or on drop, so a caller may hold it for a scope.
    /// Phases may nest; each guard times its own.
    pub fn phase(&self, name: impl Into<String>) -> Phase {
        let name = name.into();
        let started = self.now();
        if let Some(started) = started {
            self.emit_at(
                started,
                Payload::PhaseStart {
                    phase: PhaseRecord {
                        name: name.clone(),
                        duration_ms: None,
                    },
                },
            );
        }
        Phase {
            recorder: self.clone(),
            name,
            started,
            ended: AtomicBool::new(false),
        }
    }

    /// Records that the run has reached a stage, ending the stage before it.
    pub fn stage(&self, name: &str) {
        let Some(inner) = &self.inner else {
            return;
        };
        let Some(moment) = self.now() else {
            return;
        };
        let Ok(mut state) = inner.lock_state() else {
            return;
        };
        if state.ended {
            return;
        }
        if let Some((open, started)) = state.stage.take() {
            let duration = match millis_between(started, moment) {
                Ok(duration) => duration,
                Err(error) => {
                    if state.durable_failure.is_none() {
                        state.durable_failure = Some(std::io::Error::other(error));
                    }
                    return;
                }
            };
            inner.deliver(
                &mut state,
                moment,
                Payload::PhaseEnd {
                    phase: PhaseRecord {
                        name: open,
                        duration_ms: Some(duration),
                    },
                },
            );
        }
        inner.deliver(
            &mut state,
            moment,
            Payload::PhaseStart {
                phase: PhaseRecord {
                    name: name.to_owned(),
                    duration_ms: None,
                },
            },
        );
        state.stage = Some((name.to_owned(), moment));
    }

    /// Records one executed process.
    pub fn exec(&self, mut record: ExecRecord) {
        record.env_names = environment_names(&record.env_names);
        if !record.output.is_empty() {
            record.output_bytes = match u64::try_from(record.output.len()) {
                Ok(bytes) => bytes,
                Err(_unsupported_address_width) => {
                    self.fail_accounting();
                    return;
                }
            };
            record.output_sha256 = Some(hex::encode(Sha256::digest(&record.output)));
        }
        self.emit(Payload::Exec { exec: record });
    }

    /// Records a checked process record, or makes its representational failure a sticky finalization error.
    pub fn exec_result(&self, record: Result<ExecRecord, rust_mutants::trace::ExecRecordError>) {
        match record {
            Ok(record) => self.exec(record),
            Err(error) => self.fail_record(error),
        }
    }

    /// Records how far the run had got.
    pub fn progress(&self, record: ProgressRecord) {
        self.emit(Payload::Progress { progress: record });
    }

    /// Records something the run kept for a person to look at.
    pub fn artifact(&self, record: ArtifactRecord) {
        self.emit(Payload::Artifact { artifact: record });
    }

    /// Records how one mutant's tests were chosen, and what narrowed the choice.
    pub fn route(&self, record: RouteRecord) {
        self.emit(Payload::Route { route: record });
    }

    /// Records one mutant run against one target.
    pub fn mutant_exec(&self, record: MutantExecRecord) {
        self.emit(Payload::MutantExec { mutant: record });
    }

    /// Records what the probe pass measured for one target.
    pub fn probe_exec(&self, record: ProbeExecRecord) {
        self.emit(Payload::ProbeExec { probe: record });
    }

    /// Records one exchange that went past a seam.
    pub fn wire_exchange(&self, record: WireExchangeRecord) {
        self.emit(Payload::WireExchange { exchange: record });
    }

    /// Records one fault put to the suite, and what came of it.
    pub fn wire_exec(&self, record: WireExecRecord) {
        self.emit(Payload::WireExec { wire: record });
    }

    /// Records one mutant planted for a routing layer, and how the engine routed it.
    pub fn sentinel(&self, record: SentinelRecord) {
        self.emit(Payload::Sentinel { sentinel: record });
    }

    /// Records what one control established about one target's baseline reach.
    pub fn drift(&self, record: DriftRecord) {
        self.emit(Payload::Drift { drift: record });
    }

    /// Records one disposition that rested on a moved target, run again against it.
    pub fn repair(&self, record: RepairRecord) {
        self.emit(Payload::Repair { repair: record });
    }

    /// Records what one control under one knob established about one target.
    pub fn knob(&self, record: crate::report::knobs::KnobRecord) {
        self.emit(Payload::Knob { knob: record });
    }

    /// Records one closed model-checking question and its typed answer.
    pub fn model(&self, record: crate::report::ModelRecord) {
        self.emit(Payload::Model {
            model: Box::new(record),
        });
    }

    /// Records a free-form note.
    pub fn note(&self, kind: &str, detail: &str) {
        self.emit(Payload::Note {
            note: NoteRecord {
                kind: kind.to_owned(),
                detail: detail.to_owned(),
            },
        });
    }

    /// Closes the recording with the verdict, what the run counted, the error that ended it if there was one, and the event accounting, then closes the sink.
    ///
    /// # Errors
    /// The sink could not make the completed recording durable.
    pub fn run_end(
        &self,
        verdict: crate::report::Verdict,
        accounting: Option<ConclusionAccounting>,
        error: Option<String>,
    ) -> std::io::Result<()> {
        let Some(inner) = &self.inner else {
            return Ok(());
        };
        #[cfg(any(test, feature = "testkit"))]
        let moment = match inner.clock.now() {
            Ok(moment) => moment,
            Err(error) => {
                match inner.lock_state() {
                    Ok(mut state) => state.ended = true,
                    Err(_poisoned_state_is_already_a_finalization_failure) => {}
                }
                return primary_with_close(std::io::Error::other(error), inner.sink.close());
            }
        };
        #[cfg(not(any(test, feature = "testkit")))]
        let moment = inner.clock.now();
        let delivery_failure = match deliver_end(
            inner,
            EndInput {
                moment,
                verdict,
                accounting,
                error,
            },
        ) {
            Ok(EndDelivery::AlreadyEnded) => return Ok(()),
            Ok(EndDelivery::Delivered(failure)) => failure,
            Err(error) => return primary_with_close(error, inner.sink.close()),
        };
        let close_failure = match inner.sink.close() {
            Ok(()) => None,
            Err(error) => Some(error),
        };
        match (delivery_failure, close_failure) {
            (None, None) => Ok(()),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (Some(delivery), Some(close)) => Err(std::io::Error::other(format!(
                "trace delivery failed: {delivery}; closing the trace also failed: {close}"
            ))),
        }
    }

    fn now(&self) -> Option<Timestamp> {
        let inner = self.inner.as_ref()?;
        #[cfg(any(test, feature = "testkit"))]
        {
            match inner.clock.now() {
                Ok(moment) => Some(moment),
                Err(error) => {
                    self.fail_record(error);
                    None
                }
            }
        }
        #[cfg(not(any(test, feature = "testkit")))]
        {
            Some(inner.clock.now())
        }
    }

    fn emit(&self, payload: Payload) {
        if let Some(moment) = self.now() {
            self.emit_at(moment, payload);
        }
    }

    fn fail_accounting(&self) {
        let Some(inner) = &self.inner else {
            return;
        };
        match inner.lock_state() {
            Ok(mut state) => state.accounting_failed = true,
            Err(_poisoned) => {}
        }
    }

    fn fail_record(&self, error: impl std::error::Error + Send + Sync + 'static) {
        let Some(inner) = &self.inner else {
            return;
        };
        match inner.lock_state() {
            Ok(mut state) if state.durable_failure.is_none() => {
                state.durable_failure = Some(std::io::Error::other(error));
            }
            Ok(_) | Err(_) => {}
        }
    }

    /// Stamps and delivers one event under the lock, which is what keeps sequence order and delivery order the same order.
    fn emit_at(&self, moment: Timestamp, payload: Payload) {
        let Some(inner) = &self.inner else {
            return;
        };
        let Ok(mut state) = inner.lock_state() else {
            return;
        };
        if !state.ended {
            inner.deliver(&mut state, moment, payload);
        }
    }
}

fn deliver_end(inner: &Inner, input: EndInput) -> std::io::Result<EndDelivery> {
    let failure = {
        let mut state = inner.lock_state()?;
        if state.ended {
            return Ok(EndDelivery::AlreadyEnded);
        }
        finish_open_stage(inner, &mut state, input.moment);
        let (events_emitted, events_dropped) = event_accounting(inner, &mut state);
        inner.deliver(
            &mut state,
            input.moment,
            Payload::RunEnd {
                run: RunRecord {
                    verdict: input.verdict,
                    accounting: input.accounting.map(RunAccounting::from),
                    error: input.error,
                    events_emitted,
                    events_dropped,
                },
            },
        );
        state.ended = true;
        match state.durable_failure.take() {
            Some(error) => Some(error),
            None if state.accounting_failed => Some(std::io::Error::other(
                "trace event accounting overflowed or became inconsistent",
            )),
            None => None,
        }
    };
    Ok(EndDelivery::Delivered(failure))
}

fn finish_open_stage(inner: &Inner, state: &mut State, moment: Timestamp) {
    let Some((open, started)) = state.stage.take() else {
        return;
    };
    match millis_between(started, moment) {
        Ok(duration) => inner.deliver(
            state,
            moment,
            Payload::PhaseEnd {
                phase: PhaseRecord {
                    name: open,
                    duration_ms: Some(duration),
                },
            },
        ),
        Err(error) => {
            if state.durable_failure.is_none() {
                state.durable_failure = Some(std::io::Error::other(error));
            }
        }
    }
}

fn event_accounting(inner: &Inner, state: &mut State) -> (u64, u64) {
    let dropped = match inner.sink.dropped() {
        Some(dropped) => dropped,
        None => {
            state.accounting_failed = true;
            state.failures
        }
    };
    let emitted = match state.attempts.checked_sub(dropped) {
        Some(emitted) => emitted,
        None => {
            state.accounting_failed = true;
            0
        }
    };
    (emitted, dropped)
}

impl Inner {
    fn lock_state(&self) -> std::io::Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_poisoned| std::io::Error::other("the trace recorder mutex is poisoned"))
    }

    fn deliver(&self, state: &mut State, moment: Timestamp, payload: Payload) {
        let Some(seq) = state.seq.checked_add(1) else {
            state.accounting_failed = true;
            return;
        };
        let Some(attempts) = state.attempts.checked_add(1) else {
            state.accounting_failed = true;
            return;
        };
        state.seq = seq;
        state.attempts = attempts;
        let elapsed = match millis_between(self.started, moment) {
            Ok(elapsed) => elapsed,
            Err(error) => {
                if state.durable_failure.is_none() {
                    state.durable_failure = Some(std::io::Error::other(error));
                }
                return;
            }
        };
        let event = Event {
            seq: state.seq,
            timestamp: moment.to_string(),
            elapsed_ms: elapsed,
            payload,
        };
        if let Err(error) = self.sink.emit(&event) {
            match state.failures.checked_add(1) {
                Some(failures) => state.failures = failures,
                None => state.accounting_failed = true,
            }
            if self.sink.is_required() && state.durable_failure.is_none() {
                state.durable_failure = Some(error);
            }
        }
    }
}

/// The guard of a phase: ends it once, with its duration.
#[must_use = "a phase ends when its guard is dropped or ended; binding it to _ ends it at once"]
#[derive(Debug)]
pub struct Phase {
    recorder: Recorder,
    name: String,
    started: Option<Timestamp>,
    ended: AtomicBool,
}

impl Phase {
    /// Ends the phase now.
    pub fn end(self) {
        self.finish();
    }

    fn finish(&self) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        let (Some(started), Some(moment)) = (self.started, self.recorder.now()) else {
            return;
        };
        let duration = match millis_between(started, moment) {
            Ok(duration) => duration,
            Err(error) => {
                self.recorder.fail_record(error);
                return;
            }
        };
        self.recorder.emit_at(
            moment,
            Payload::PhaseEnd {
                phase: PhaseRecord {
                    name: self.name.clone(),
                    duration_ms: Some(duration),
                },
            },
        );
    }
}

impl Drop for Phase {
    fn drop(&mut self) {
        self.finish();
    }
}

fn millis_between(from: Timestamp, to: Timestamp) -> Result<u64, ClockError> {
    u64::try_from(to.duration_since(from).as_millis()).map_err(ClockError::ElapsedOutsideWire)
}

fn primary_with_close(primary: std::io::Error, close: std::io::Result<()>) -> std::io::Result<()> {
    match close {
        Ok(()) => Err(primary),
        Err(close) => Err(std::io::Error::other(format!(
            "trace finalization failed: {primary}; closing the trace also failed: {close}"
        ))),
    }
}

/// The sorted, deduplicated variable names of an environment description.
/// Entries arriving as `NAME=value` are reduced to `NAME`, which is the only half a trace is allowed to keep.
fn environment_names(entries: &[String]) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .split_once('=')
                .map_or(entry.as_str(), |(name, _)| name)
        })
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}
