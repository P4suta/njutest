// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine trace: what a run did while it did it, as diagnostic exhaust.
//!
//! The rules are those of ADR 0002. A trace is never evidence: it takes no
//! part in any claim, never costs the run (sink failures are counted, never
//! returned), keeps no secret (an exec event carries environment variable
//! names alone, and output is digested rather than serialized), and is
//! honest about loss (every recording ends with `events_emitted` and
//! `events_dropped`).
//!
//! The disabled trace is [`Recorder::disabled`]. Call sites record
//! unconditionally, which keeps the traced and untraced paths identical; the
//! disabled recorder costs a branch on an `Option`.

mod event;
mod reader;
mod sink;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

pub use event::{
    DiscoverFileRecord, Event, ExecRecord, NoteRecord, OpenRecord, Payload, PhaseRecord, RunRecord,
    SCHEMA, SiteRecord, SkipCount, SnapshotRecord, SweepRecord,
};
pub use reader::{Problem, ReadError, check, read_events};
pub use sink::{
    DirSink, FILE_NAME, MemorySink, OUTPUT_DIRECTORY_NAME, OUTPUT_FILE_LIMIT, Sink,
    TRUNCATION_MARKER, WriterSink,
};

/// A source of the current moment: the recorder's one seam, so a test can
/// freeze time and a golden can freeze the wire shape.
pub type Clock = Box<dyn Fn() -> Timestamp + Send + Sync>;

/// Turns the events of a run into a stream a sink keeps.
///
/// Cloning shares the recording: every clone records into the same stream,
/// and sequencing and delivery happen under one lock, so events reach the
/// sink in strictly increasing sequence order however many threads record
/// at once.
#[derive(Clone)]
pub struct Recorder {
    inner: Option<Arc<Inner>>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("enabled", &self.inner.is_some())
            .finish()
    }
}

struct Inner {
    sink: Box<dyn Sink>,
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
}

impl Recorder {
    /// The trace that records nothing.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Starts a recording into `sink`, reading the moment from `clock`, and
    /// emits its `run-start` event.
    #[must_use]
    pub fn new(sink: Box<dyn Sink>, clock: Clock) -> Self {
        let started = clock();
        let inner = Arc::new(Inner {
            sink,
            clock,
            started,
            state: Mutex::new(State::default()),
        });
        let recorder = Self { inner: Some(inner) };
        recorder.emit_at(
            started,
            Payload::RunStart {
                schema: SCHEMA.to_owned(),
                engine: crate::VERSION.to_owned(),
            },
        );
        recorder
    }

    /// [`Recorder::new`] on the wall clock.
    #[must_use]
    pub fn wall(sink: Box<dyn Sink>) -> Self {
        Self::new(sink, Box::new(Timestamp::now))
    }

    /// Whether anything is recorded.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Records the beginning of a phase and returns the guard that ends it.
    /// The guard ends the phase once, on [`Phase::end`] or on drop, so a
    /// caller may hold it for a scope. Phases may nest; each guard times its
    /// own.
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

    /// Records an opened workspace.
    pub fn open(&self, record: OpenRecord) {
        self.emit(Payload::Open { open: record });
    }

    /// Records a snapshot, taken or refused.
    pub fn snapshot(&self, record: SnapshotRecord) {
        self.emit(Payload::Snapshot { snapshot: record });
    }

    /// Records the decisions discovery took in one file.
    pub fn discover_file(&self, record: DiscoverFileRecord) {
        self.emit(Payload::DiscoverFile { discover: record });
    }

    /// Records one executed process.
    ///
    /// Environment entries are reduced to their names, sorted and
    /// deduplicated, and the capture is digested, so the event carries the
    /// shape of the execution and none of its secrets. The raw capture rides
    /// along for a sink that preserves it and is never serialized.
    pub fn exec(&self, mut record: ExecRecord) {
        record.env_names = environment_names(&record.env_names);
        if !record.output.is_empty() {
            record.output_bytes = u64::try_from(record.output.len()).unwrap_or(u64::MAX);
            record.output_sha256 = Some(hex::encode(Sha256::digest(&record.output)));
        }
        self.emit(Payload::Exec { exec: record });
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

    /// Closes the recording with the outcome, the error that ended the run
    /// if there was one, and the event accounting, then closes the sink.
    ///
    /// `events_emitted` counts the events the sink kept and `events_dropped`
    /// the ones it could not. A sink that reports its own drops is the
    /// authority; otherwise the recorder counts the emissions that failed. A
    /// recording ends once, and anything recorded afterwards is not kept.
    pub fn run_end(&self, outcome: &str, error: Option<String>) {
        let Some(inner) = &self.inner else {
            return;
        };
        let moment = (inner.clock)();
        {
            let mut state = inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.ended {
                return;
            }
            let events_dropped = inner.sink.dropped().unwrap_or(state.failures);
            let events_emitted = state.attempts.saturating_sub(events_dropped);
            inner.deliver(
                &mut state,
                moment,
                Payload::RunEnd {
                    run: RunRecord {
                        outcome: outcome.to_owned(),
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
            .map_or_else(Timestamp::now, |inner| (inner.clock)())
    }

    fn emit(&self, payload: Payload) {
        let moment = self.now();
        self.emit_at(moment, payload);
    }

    /// Stamps and delivers one event under the lock, which is what keeps
    /// sequence order and delivery order the same order.
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

/// The sorted, deduplicated variable names of an environment description.
/// Entries arriving as `NAME=value` are reduced to `NAME`, which is the only
/// half a trace is allowed to keep.
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
