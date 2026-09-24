// SPDX-FileCopyrightText: 2026 njutest contributors
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
    AttributionRecord, BisectRecord, BuildRecord, CacheRecord, DelayRecord, DischargeRecord,
    DiscoverFileRecord, EVERY_TYPE, Event, EvidenceRecord, ExecRecord, IdenticalRecord,
    InstrumentRecord, KeptRecord, Measurement, MutantExecRecord, NjutestBuild, NjutestBuildError,
    NoteRecord, OpenRecord, Payload, PerturbationRecord, PerturbedRecord, PhaseRecord, ReachRecord,
    RouteRecord, RunRecord, SCHEMA, SelectRecord, SetRecord, SiteRecord, SkipClaimRecord,
    SkipCount, SnapshotRecord, SummaryRecord, SweepRecord, TargetRecord, TouchRecord, TraceContext,
    ValidateRoundRecord, VerifyRecord, WitnessRecord,
};
pub use reader::{Problem, ReadError, check, read_events};
pub use sink::{
    ChannelSink, DirSink, FILE_NAME, MemorySink, OUTPUT_DIRECTORY_NAME, OUTPUT_FILE_LIMIT,
    ObserverState, Sink, TRUNCATION_MARKER,
};

/// Why a supervised process record cannot be represented exactly by the trace wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExecRecordError {
    /// Whole milliseconds did not fit the wire's u64 field.
    #[error("the {field} duration does not fit the trace wire's u64 milliseconds")]
    MillisecondsOutsideWire {
        /// Which duration could not be represented.
        field: &'static str,
    },
    /// A platform string cannot be encoded by the trace's UTF-8 string field.
    #[error("the {field} value is not valid UTF-8")]
    NonUtf8 {
        /// Which string field could not be represented.
        field: &'static str,
    },
}

fn trace_milliseconds(
    duration: std::time::Duration,
    field: &'static str,
) -> Result<u64, ExecRecordError> {
    u64::try_from(duration.as_millis())
        .map_err(|_overflow| ExecRecordError::MillisecondsOutsideWire { field })
}

fn trace_text(value: &std::ffi::OsStr, field: &'static str) -> Result<String, ExecRecordError> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or(ExecRecordError::NonUtf8 { field })
}

fn trace_env_names(
    env: Option<&[(std::ffi::OsString, std::ffi::OsString)]>,
) -> Result<Vec<String>, ExecRecordError> {
    let Some(env) = env else {
        return Ok(Vec::new());
    };
    env.iter()
        .map(|(key, _value)| trace_text(key, "environment name"))
        .collect()
}

impl ExecRecord {
    /// The record of one supervised run: the spec's command line, directory, environment names, and timeout, and the result's exit code, timeout flag, duration, output, and error.
    /// The recorder digests the output and strips the environment values on emission.
    ///
    /// # Errors
    /// A timeout or measured duration does not fit the trace wire exactly.
    pub fn of(
        spec: &crate::runner::Spec,
        result: &crate::runner::RunResult,
    ) -> Result<Self, ExecRecordError> {
        Ok(Self {
            argv: spec
                .argv
                .iter()
                .map(|arg| trace_text(arg, "argument"))
                .collect::<Result<Vec<_>, _>>()?,
            dir: spec
                .dir
                .as_ref()
                .map(|dir| trace_text(dir.as_os_str(), "working directory"))
                .transpose()?,
            env_names: trace_env_names(spec.env.as_deref())?,
            timeout_ms: spec
                .timeout
                .map(|timeout| trace_milliseconds(timeout, "timeout"))
                .transpose()?,
            quiet_ms: spec
                .progress
                .as_ref()
                .map(|progress| trace_milliseconds(progress.quiet, "quiet window"))
                .transpose()?,
            stopped: crate::execute::Stopped::of(result),
            duration_ms: trace_milliseconds(result.duration, "measured process")?,
            output_bytes: 0,
            output_sha256: None,
            output_truncated: false,
            output_path: None,
            error: result
                .termination
                .error()
                .map(|failure| failure.to_string()),
            output: result.output.clone(),
        })
    }
}

/// A source of the current moment: the recorder's one seam, so a test can freeze time and a golden can freeze the wire shape.
#[derive(Debug)]
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

#[derive(Debug, thiserror::Error)]
enum ClockError {
    #[error("the deterministic trace clock exhausted its u64 reading counter")]
    ReadingsExhausted,
    #[error("the deterministic trace clock reading does not fit its u32 step multiplier")]
    StepOutsideRange(#[source] std::num::TryFromIntError),
    #[error("the deterministic trace clock duration overflowed")]
    DurationOverflow,
    #[error("the deterministic trace clock left the timestamp range")]
    TimestampOutsideRange,
    #[error("the elapsed trace duration does not fit the wire's u64 milliseconds")]
    ElapsedOutsideWire(#[source] std::num::TryFromIntError),
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

    fn start(&self) -> Timestamp {
        match self {
            Self::Wall => Timestamp::now(),
            Self::Stepping {
                origin, readings, ..
            } => {
                readings.store(1, Ordering::SeqCst);
                *origin
            }
        }
    }

    fn now(&self) -> Result<Timestamp, ClockError> {
        match self {
            Self::Wall => Ok(Timestamp::now()),
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
}

impl State {
    const fn observed_drops(&mut self, dropped: Option<u64>) -> u64 {
        let Some(dropped) = dropped else {
            self.accounting_failed = true;
            return self.failures;
        };
        dropped
    }

    const fn emitted(&mut self, dropped: u64) -> u64 {
        let Some(emitted) = self.attempts.checked_sub(dropped) else {
            self.accounting_failed = true;
            return 0;
        };
        emitted
    }
}

impl Recorder {
    /// The trace that records nothing.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Starts a recording into `sink`, reading the moment from `clock`, and emits its `run-start` event.
    #[must_use]
    pub fn new(sink: Sink, clock: Clock, context: TraceContext) -> Self {
        let started = clock.start();
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
                context,
            },
        );
        recorder
    }

    /// [`Recorder::new`] on the wall clock.
    #[must_use]
    pub fn wall(sink: Sink, context: TraceContext) -> Self {
        Self::new(sink, Clock::Wall, context)
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

    /// Injects a durable-write failure after recording has started.
    #[cfg(any(test, feature = "testkit"))]
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
    pub fn exec_result(&self, record: Result<ExecRecord, ExecRecordError>) {
        match record {
            Ok(record) => self.exec(record),
            Err(error) => self.fail_record(error),
        }
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

    /// Records what one control started under a perturbation came to.
    pub fn perturbed(&self, record: PerturbedRecord) {
        self.emit(Payload::PerturbedControl { perturbed: record });
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
    ///
    /// # Errors
    /// The sink could not make the completed recording durable.
    pub fn run_end(&self, outcome: &str, error: Option<String>) -> std::io::Result<()> {
        let Some(inner) = &self.inner else {
            return Ok(());
        };
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
        let delivery_failure = {
            let mut state = match inner.lock_state() {
                Ok(state) => state,
                Err(error) => {
                    return primary_with_close(error, inner.sink.close());
                }
            };
            if state.ended {
                return Ok(());
            }
            let events_dropped = state.observed_drops(inner.sink.dropped());
            let events_emitted = state.emitted(events_dropped);
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
            match state.durable_failure.take() {
                Some(error) => Some(error),
                None if state.accounting_failed => Some(std::io::Error::other(
                    "trace event accounting overflowed or became inconsistent",
                )),
                None => None,
            }
        };
        match (delivery_failure, inner.sink.close()) {
            (None, Ok(())) => Ok(()),
            (Some(error), Ok(())) | (None, Err(error)) => Err(error),
            (Some(delivery), Err(close)) => Err(std::io::Error::other(format!(
                "trace delivery failed: {delivery}; closing the trace also failed: {close}"
            ))),
        }
    }

    fn now(&self) -> Option<Timestamp> {
        let Some(inner) = self.inner.as_ref() else {
            return Some(Timestamp::now());
        };
        match inner.clock.now() {
            Ok(moment) => Some(moment),
            Err(error) => {
                self.fail_record(error);
                None
            }
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
