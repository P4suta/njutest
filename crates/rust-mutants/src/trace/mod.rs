// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine trace: what a run did while it did it, as diagnostic exhaust.

mod event;
mod reader;
mod sink;
pub mod summary;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

pub use event::{
    AttributionRecord, BisectRecord, BuildRecord, CacheRecord, DischargeRecord, DiscoverFileRecord,
    EVERY_TYPE, Event, EvidenceRecord, ExecRecord, IdenticalRecord, InstrumentRecord, KeptRecord,
    MutantExecRecord, NoteRecord, OpenRecord, Payload, PhaseRecord, ProbeExecRecord, RouteRecord,
    RunRecord, SCHEMA, SelectRecord, SiteRecord, SkipClaimRecord, SkipCount, SnapshotRecord,
    SweepRecord, TargetRecord, TouchRecord, ValidateRoundRecord, VerifyRecord, WitnessRecord,
};
pub use reader::{Problem, ReadError, check, read_events};
pub use sink::{
    ChannelSink, DirSink, FILE_NAME, MemorySink, OUTPUT_DIRECTORY_NAME, OUTPUT_FILE_LIMIT, Sink,
    TRUNCATION_MARKER,
};

impl ExecRecord {
    /// The record of one supervised run: the spec's command line, directory, environment names, and timeout, and the result's exit code, timeout flag, duration, output, and error. The recorder digests the output and strips the environment values on emission.
    #[must_use]
    pub fn of(spec: &crate::runner::Spec, result: &crate::runner::RunResult) -> Self {
        Self {
            argv: spec
                .argv
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            dir: spec
                .dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            env_names: spec
                .env
                .as_ref()
                .map(|env| {
                    env.iter()
                        .map(|(key, _)| key.to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default(),
            timeout_ms: spec
                .timeout
                .map(|timeout| u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
            exit_code: result.exit_code,
            timed_out: result.timed_out,
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
            output_bytes: 0,
            output_sha256: None,
            output_truncated: false,
            output_path: None,
            error: result.error.as_ref().map(ToString::to_string),
            output: result.output.clone(),
        }
    }
}

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
        step: std::time::Duration,
        /// How many readings there have been.
        readings: std::sync::atomic::AtomicU64,
    },
}

impl Clock {
    /// A clock that starts at `origin` and advances by `step` per reading.
    #[must_use]
    pub const fn stepping(origin: Timestamp, step: std::time::Duration) -> Self {
        Self::Stepping {
            origin,
            step,
            readings: std::sync::atomic::AtomicU64::new(0),
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
}

impl Recorder {
    /// The trace that records nothing.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Starts a recording into `sink`, reading the moment from `clock`, and emits its `run-start` event.
    #[must_use]
    pub fn new(sink: Sink, clock: Clock) -> Self {
        let started = clock.now();
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
    pub fn wall(sink: Sink) -> Self {
        Self::new(sink, Clock::Wall)
    }

    /// Whether the sink was released, which `run_end` does once.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.sink.is_closed())
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
    pub fn exec(&self, mut record: ExecRecord) {
        record.env_names = environment_names(&record.env_names);
        if !record.output.is_empty() {
            record.output_bytes = u64::try_from(record.output.len()).unwrap_or(u64::MAX);
            record.output_sha256 = Some(hex::encode(Sha256::digest(&record.output)));
        }
        self.emit(Payload::Exec { exec: record });
    }

    /// Records one file rewritten to hold its mutants.
    pub fn instrument(&self, record: InstrumentRecord) {
        self.emit(Payload::Instrument { instrument: record });
    }

    /// Records one validation round.
    pub fn validate_round(&self, record: ValidateRoundRecord) {
        self.emit(Payload::ValidateRound { round: record });
    }

    /// Records one narrowing of suspects by halving.
    pub fn bisect(&self, record: BisectRecord) {
        self.emit(Payload::Bisect { bisect: record });
    }

    /// Records the test binaries a build produced.
    pub fn build(&self, record: BuildRecord) {
        self.emit(Payload::Build { build: record });
    }

    /// Records one target run with nothing active.
    pub fn verify(&self, record: VerifyRecord) {
        self.emit(Payload::Verify { verify: record });
    }

    /// Records what one target's guards said they reached.
    pub fn touch(&self, record: TouchRecord) {
        self.emit(Payload::Touch { touch: record });
    }

    /// Records one target run against the probe tree.
    pub fn probe_exec(&self, record: ProbeExecRecord) {
        self.emit(Payload::ProbeExec { probe: record });
    }

    /// Records one branch claim put to the compiler.
    pub fn witness(&self, record: WitnessRecord) {
        self.emit(Payload::Witness { witness: record });
    }

    /// Records one directory the run kept rather than removed.
    pub fn kept(&self, record: KeptRecord) {
        self.emit(Payload::Kept { kept: record });
    }

    /// Records one `rust-mutants: skip` marker, and whether it hid anything.
    pub fn skip_claim(&self, record: SkipClaimRecord) {
        self.emit(Payload::SkipClaim { claim: record });
    }

    /// Records how one mutant's targets were chosen, and which of them ran.
    pub fn route(&self, record: RouteRecord) {
        self.emit(Payload::Route { route: record });
    }

    /// Records what the outcome store was asked about one mutant.
    pub fn cache(&self, record: CacheRecord) {
        self.emit(Payload::Cache { cache: record });
    }

    /// Records why one mutant was never executed.
    pub fn select(&self, record: SelectRecord) {
        self.emit(Payload::Select { select: record });
    }

    /// Records what the equivalence layer said about one mutation.
    pub fn identical(&self, record: IdenticalRecord) {
        self.emit(Payload::Identical { identical: record });
    }

    /// Records one file a run kept for an audit.
    pub fn evidence(&self, record: EvidenceRecord) {
        self.emit(Payload::Evidence { evidence: record });
    }

    /// Records one mutant executed against one target.
    pub fn mutant_exec(&self, record: MutantExecRecord) {
        self.emit(Payload::MutantExec { mutant: record });
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

    /// Closes the recording with the outcome, the error that ended the run if there was one, and the event accounting, then closes the sink.
    pub fn run_end(&self, outcome: &str, error: Option<String>) {
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
