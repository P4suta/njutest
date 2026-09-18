// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runner's trace: what a run did while it did it, as diagnostic exhaust.

mod event;
mod reader;
mod sink;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

pub use event::{
    ArtifactRecord, AskedRecord, DischargeRecord, Event, ExecRecord, MutantExecRecord, NoteRecord,
    Payload, PhaseRecord, ProbeExecRecord, ProgressRecord, Read, RouteRecord, RunRecord, SCHEMA,
    StartRecord, WireExchangeRecord, WireExecRecord,
};
pub use reader::{Problem, ReadError, check, read_events};
pub use sink::{
    DirSink, FILE_NAME, MemorySink, OUTPUT_DIRECTORY_NAME, OUTPUT_FILE_LIMIT, RING_CAPACITY, Sink,
    TRUNCATION_MARKER,
};

use crate::report::Accounting;

/// A source of the current moment: the recorder's one seam, so a test can freeze time and a golden can freeze the wire shape.
#[derive(Debug)]
#[non_exhaustive]
pub enum Clock {
    /// The moment it actually is.
    Wall,
    /// One that starts at `origin` and advances by `step` each reading, so a recording is the same bytes every time it is made.
    Stepping {
        /// The first moment it answers with.
        origin: Timestamp,
        /// How far it moves per reading.
        step: Duration,
        /// How many readings there have been.
        readings: AtomicU64,
    },
}

impl Clock {
    /// A clock that starts at `origin` and advances by `step` per reading.
    #[must_use]
    pub const fn stepping(origin: Timestamp, step: Duration) -> Self {
        Self::Stepping {
            origin,
            step,
            readings: AtomicU64::new(0),
        }
    }

    /// The moment now, by this clock.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        match self {
            Self::Wall => Timestamp::now(),
            Self::Stepping {
                origin,
                step,
                readings,
            } => {
                let reading = readings.fetch_add(1, Ordering::SeqCst);
                let elapsed = step.saturating_mul(u32::try_from(reading).unwrap_or(u32::MAX));
                origin.checked_add(elapsed).unwrap_or(*origin)
            }
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
    ended: bool,
    /// The stage the run said it was in, and when it said so.
    stage: Option<(String, Timestamp)>,
}

impl Recorder {
    /// The trace that records nothing.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Starts a recording into `sink`, reading the moment from `clock`, and emits its `run-start` event.
    #[must_use]
    pub fn new(sink: Sink, clock: Clock, start: StartRecord) -> Self {
        let started = clock.now();
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

    /// [`Recorder::new`] on the wall clock.
    #[must_use]
    pub fn wall(sink: Sink, start: StartRecord) -> Self {
        Self::new(sink, Clock::Wall, start)
    }

    /// Every event a memory sink of this recording kept, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.inner
            .as_ref()
            .map(|inner| inner.sink.events())
            .unwrap_or_default()
    }

    /// Whether anything is recorded.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Records the beginning of a phase and returns the guard that ends it. The guard ends the phase once, on [`Phase::end`] or on drop, so a caller may hold it for a scope. Phases may nest; each guard times its own.
    pub fn phase(&self, name: impl Into<String>) -> Phase {
        let name = name.into();
        let started = self.now();
        self.emit_at(
            started,
            Payload::PhaseStart {
                phase: PhaseRecord {
                    name: name.clone(),
                    duration_ms: None,
                },
            },
        );
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
        let moment = inner.clock.now();
        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.ended {
            return;
        }
        if let Some((open, started)) = state.stage.take() {
            let duration = millis_between(started, moment);
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
            record.output_bytes = u64::try_from(record.output.len()).unwrap_or(u64::MAX);
            record.output_sha256 = Some(hex::encode(Sha256::digest(&record.output)));
        }
        self.emit(Payload::Exec { exec: record });
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
    pub fn run_end(&self, verdict: &str, accounting: Option<Accounting>, error: Option<String>) {
        let Some(inner) = &self.inner else {
            return;
        };
        let moment = inner.clock.now();
        {
            let mut state = inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.ended {
                return;
            }
            if let Some((open, started)) = state.stage.take() {
                let duration = millis_between(started, moment);
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
            let events_dropped = inner.sink.dropped().unwrap_or(state.failures);
            let events_emitted = state.attempts.saturating_sub(events_dropped);
            inner.deliver(
                &mut state,
                moment,
                Payload::RunEnd {
                    run: RunRecord {
                        verdict: verdict.to_owned(),
                        accounting,
                        error,
                        events_emitted,
                        events_dropped,
                    },
                },
            );
            state.ended = true;
        }
        drop(inner.sink.close());
    }

    fn now(&self) -> Timestamp {
        self.inner
            .as_ref()
            .map_or_else(Timestamp::now, |inner| inner.clock.now())
    }

    fn emit(&self, payload: Payload) {
        let moment = self.now();
        self.emit_at(moment, payload);
    }

    /// Stamps and delivers one event under the lock, which is what keeps sequence order and delivery order the same order.
    fn emit_at(&self, moment: Timestamp, payload: Payload) {
        let Some(inner) = &self.inner else {
            return;
        };
        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.ended {
            inner.deliver(&mut state, moment, payload);
        }
    }
}

impl Inner {
    fn deliver(&self, state: &mut State, moment: Timestamp, payload: Payload) {
        state.seq = state.seq.saturating_add(1);
        state.attempts = state.attempts.saturating_add(1);
        let elapsed = millis_between(self.started, moment);
        let event = Event {
            seq: state.seq,
            timestamp: moment.to_string(),
            elapsed_ms: elapsed,
            payload,
        };
        if self.sink.emit(&event).is_err() {
            state.failures = state.failures.saturating_add(1);
        }
    }
}

/// The guard of a phase: ends it once, with its duration.
#[must_use = "a phase ends when its guard is dropped or ended; binding it to _ ends it at once"]
#[derive(Debug)]
pub struct Phase {
    recorder: Recorder,
    name: String,
    started: Timestamp,
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
        let moment = self.recorder.now();
        let duration = millis_between(self.started, moment);
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

/// Whole milliseconds from `from` to `to`, zero when the clock went backwards.
fn millis_between(from: Timestamp, to: Timestamp) -> u64 {
    u64::try_from(to.duration_since(from).as_millis()).unwrap_or(0)
}

/// The sorted, deduplicated variable names of an environment description. Entries arriving as `NAME=value` are reduced to `NAME`, which is the only half a trace is allowed to keep.
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
