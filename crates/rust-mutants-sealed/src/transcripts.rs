// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one sealed invocation established, remembered under the digest of everything it was a function of, so a later invocation of the same module under the same world does not run it again.

use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::digest::SealedDigest;
use crate::transcript::Transcript;

/// The name of the shape.
pub const SCHEMA: &str = "rust-mutants-sealed-transcript-v1";

/// The directory records live in, below the user's cache directory.
pub const LAYOUT: &str = "rust-mutants/sealed-transcripts-v1";

/// One remembered transcript, as a record on a disk.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Remembered {
    /// [`SCHEMA`].
    schema: String,
    /// The digest of everything the invocation was a function of, as the record is named by it.
    invocation: String,
    /// What it established.
    transcript: Transcript,
}

/// A store of sealed transcripts, or no store where a run establishes everything afresh.
#[derive(Debug, Clone, Default)]
pub struct Transcripts {
    /// Where records live, or nothing.
    root: Option<PathBuf>,
}

impl Transcripts {
    /// A store whose records live under `root`, or a store of nothing where `root` is none.
    #[must_use]
    pub fn under(root: Option<&Path>) -> Self {
        Self {
            root: root.map(Path::to_path_buf),
        }
    }

    /// What one invocation established, where a record of it says so and reads back as itself: nothing where there is no store, no record, or one whose bytes are not what they say they are.
    #[must_use]
    pub fn recall(&self, invocation: &SealedDigest) -> Option<Transcript> {
        let root = self.root.as_ref()?;
        let path = path_of(root, invocation);
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(_absent) => return None,
        };
        let Ok(strict) = crate::strictjson::decode_slice::<Remembered>(&raw) else {
            return None;
        };
        if strict.schema != SCHEMA || strict.invocation != invocation.to_string() {
            return None;
        }
        strict.transcript.checked(invocation)
    }

    /// Remembers one transcript; an unwritable record is a later miss, never a failure of this run.
    pub fn remember(&self, invocation: &SealedDigest, transcript: &Transcript) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        let record = Remembered {
            schema: SCHEMA.to_owned(),
            invocation: invocation.to_string(),
            transcript: transcript.clone(),
        };
        let Ok(mut raw) = serde_json::to_vec(&record) else {
            return;
        };
        raw.push(b'\n');
        match std::fs::create_dir_all(root)
            .and_then(|()| write_once(&path_of(root, invocation), &raw))
        {
            Ok(()) => {}
            Err(_unwritable) => {}
        }
    }
}

/// Where the record of the invocation `digest` names is kept.
fn path_of(root: &Path, digest: &SealedDigest) -> PathBuf {
    root.join(format!("{digest}.json"))
}

/// Writes `bytes` to `path` once, whole, so a reader never sees half a record.
fn write_once(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("a cache record has no directory"))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged
        .persist(path)
        .map(|_file| ())
        .map_err(|source| source.error)
}

/// How one module preparation request was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reuse {
    /// An actual preparation compiled the module, because no held code answered.
    Cold,
    /// Wasmtime's content-addressed cache held the compiled code, which the preparation loaded.
    Disk,
    /// The process's own held module, prepared on a compatible engine, answered.
    Process,
}

/// Shared counts of modules prepared, instances started, and invocations a record answered.
#[derive(Debug, Clone, Default)]
pub struct Counted {
    inner: Arc<Countings>,
}

/// The counters themselves, behind the shared handle.
#[derive(Debug, Default)]
struct Countings {
    /// Actual preparation requests, indexed by their physical preparation key.
    modules: Mutex<BTreeMap<String, ModuleWork>>,
    /// Whether any bench of the run assembled, which is what says a run that counted nothing sealed anything at all.
    assembled: AtomicBool,
    /// Module preparation requests this run's preparations answered, including every kind of reuse.
    compiles: AtomicU64,
    compilation_measured: AtomicBool,
    /// Preparations answered by compiled code already held, on Wasmtime's cache or by this process.
    hits: AtomicU64,
    /// Of the hits, the ones this process's own held module answered.
    process: AtomicU64,
    /// Preparations that compiled the module because no held code answered.
    misses: AtomicU64,
    /// Actual `Module::new` preparation attempts, including ones that then failed.
    attempts: AtomicU64,
    failed_cold: AtomicU64,
    failed_disk: AtomicU64,
    /// Preparation attempts that failed, which hold nothing reusable.
    failures: AtomicU64,
    compile_ns: AtomicU64,
    execution_measured: AtomicBool,
    execution_ns: AtomicU64,
    /// Instances started on the host.
    instances: AtomicU64,
    /// Invocations a record answered.
    answered: AtomicU64,
}

impl Counted {
    /// Says a bench assembled over this run's counters.
    pub fn assembled(&self) {
        self.inner.assembled.store(true, Ordering::Relaxed);
    }

    /// Says one module preparation request was answered.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if the count exceeds its recorded width.
    pub fn compiled(&self) -> Result<(), crate::SealedError> {
        counted(&self.inner.compiles)
    }

    /// Records one module preparation request: the work its answer cost and how it was answered.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if a count or elapsed time exceeds its recorded width.
    pub fn prepared(&self, duration: Duration, reuse: Reuse) -> Result<(), crate::SealedError> {
        counted(&self.inner.compiles)?;
        match reuse {
            Reuse::Cold => {
                counted(&self.inner.misses)?;
                counted(&self.inner.attempts)?;
            }
            Reuse::Disk => {
                counted(&self.inner.hits)?;
                counted(&self.inner.attempts)?;
            }
            Reuse::Process => {
                counted(&self.inner.hits)?;
                counted(&self.inner.process)?;
            }
        }
        elapsed(&self.inner.compile_ns, duration)?;
        self.inner
            .compilation_measured
            .store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Records the actual keyed request together with the existing aggregate accounting.
    pub(crate) fn prepared_module(
        &self,
        (module, configuration): (&SealedDigest, &SealedDigest),
        duration: Duration,
        reuse: Reuse,
    ) -> Result<(), crate::SealedError> {
        self.observe_module(
            module,
            configuration,
            ModuleAnswer::Prepared(duration, reuse),
        )?;
        self.prepared(duration, reuse)
    }

    /// Records a refused request, distinguishing validation from an actual failed attempt.
    pub(crate) fn failed_module(
        &self,
        module: &SealedDigest,
        configuration: &SealedDigest,
        attempt: Option<(Duration, bool)>,
    ) -> Result<(), crate::SealedError> {
        self.observe_module(module, configuration, ModuleAnswer::Refused(attempt))?;
        if let Some((duration, disk)) = attempt {
            self.attempt_failed(duration, disk)?;
        }
        self.failed()
    }

    /// Updates one observation under its owned lock without publishing a partial width failure.
    fn observe_module(
        &self,
        module: &SealedDigest,
        configuration: &SealedDigest,
        answer: ModuleAnswer,
    ) -> Result<(), crate::SealedError> {
        let mut modules =
            self.inner
                .modules
                .lock()
                .map_err(|_poisoned| crate::SealedError::HostInvariant {
                    invariant: crate::error::Invariant::ModuleWorkPoisoned,
                })?;
        let key = crate::preparation_key(module, configuration).to_string();
        let observed = modules
            .entry(key)
            .or_insert_with(|| ModuleWork::new(module, configuration));
        let mut next = observed.clone();
        next.observe(answer)?;
        *observed = next;
        drop(modules);
        Ok(())
    }

    /// Records one actual preparation attempt that failed, with the work it had done when it failed.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if a count or elapsed time exceeds its recorded width.
    pub fn attempt_failed(&self, work: Duration, disk: bool) -> Result<(), crate::SealedError> {
        counted(&self.inner.attempts)?;
        if disk {
            counted(&self.inner.failed_disk)?;
        } else {
            counted(&self.inner.failed_cold)?;
        }
        elapsed(&self.inner.compile_ns, work)?;
        self.inner
            .compilation_measured
            .store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Says one module preparation request was refused, however far it had gone.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if the count exceeds its recorded width.
    pub fn failed(&self) -> Result<(), crate::SealedError> {
        counted(&self.inner.failures)
    }

    /// Records elapsed host time for an invocation that actually executed.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if elapsed time exceeds its recorded width.
    pub fn executed(&self, duration: Duration) -> Result<(), crate::SealedError> {
        elapsed(&self.inner.execution_ns, duration)?;
        self.inner.execution_measured.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Says the host started one instance.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if the count exceeds its recorded width.
    pub fn instantiated(&self) -> Result<(), crate::SealedError> {
        counted(&self.inner.instances)
    }

    /// Says one invocation was answered by a record.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if the count exceeds its recorded width.
    pub fn answered(&self) -> Result<(), crate::SealedError> {
        counted(&self.inner.answered)
    }

    /// What the run spent, or nothing where no bench assembled and so nothing was counted.
    #[must_use]
    pub fn spent(&self) -> Option<Spent> {
        if !self.inner.assembled.load(Ordering::Relaxed) {
            return None;
        }
        let process = self.inner.process.load(Ordering::Relaxed);
        let failures = self.inner.failures.load(Ordering::Relaxed);
        let attempts = self.inner.attempts.load(Ordering::Relaxed);
        let failed_cold = self.inner.failed_cold.load(Ordering::Relaxed);
        let failed_disk = self.inner.failed_disk.load(Ordering::Relaxed);
        let modules = match self.inner.modules.lock() {
            Ok(modules) => (!modules.is_empty()).then(|| modules.clone()),
            Err(_poisoned) => {
                eprintln!("{}", crate::error::Invariant::ModuleWorkPoisoned);
                std::process::abort();
            }
        };
        Some(Spent {
            modules,
            compiles: self.inner.compiles.load(Ordering::Relaxed),
            instances: self.inner.instances.load(Ordering::Relaxed),
            answered: self.inner.answered.load(Ordering::Relaxed),
            compilation: self
                .inner
                .compilation_measured
                .load(Ordering::Relaxed)
                .then(|| Compilation {
                    hits: self.inner.hits.load(Ordering::Relaxed),
                    misses: self.inner.misses.load(Ordering::Relaxed),
                    duration_ns: self.inner.compile_ns.load(Ordering::Relaxed),
                    process: (process > 0).then_some(process),
                    attempts: (attempts > 0).then_some(attempts),
                    failed_cold: (failed_cold > 0).then_some(failed_cold),
                    failed_disk: (failed_disk > 0).then_some(failed_disk),
                }),
            execution_ns: self
                .inner
                .execution_measured
                .load(Ordering::Relaxed)
                .then(|| self.inner.execution_ns.load(Ordering::Relaxed)),
            failures: (failures > 0).then_some(failures),
        })
    }
}

/// Adds an elapsed duration without truncating or clamping it.
fn elapsed(counter: &AtomicU64, duration: Duration) -> Result<(), crate::SealedError> {
    let nanos = u64::try_from(duration.as_nanos()).map_err(|_overflow| {
        crate::SealedError::HostInvariant {
            invariant: crate::error::Invariant::Width,
        }
    })?;
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(nanos)
        })
        .map(|_previous| ())
        .map_err(|_overflow| crate::SealedError::HostInvariant {
            invariant: crate::error::Invariant::Width,
        })
}

/// Adds one without letting an exhausted count impersonate a fresh one.
fn counted(counter: &AtomicU64) -> Result<(), crate::SealedError> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map(|_previous| ())
        .map_err(|_exhausted| crate::SealedError::HostInvariant {
            invariant: crate::error::Invariant::Width,
        })
}

/// What the host spent on sealed executions, as a recording writes it: the module preparations answered, the instances started, and the invocations a record answered instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spent {
    /// Physical keyed module requests and attempts, absent when these were not observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules: Option<BTreeMap<String, ModuleWork>>,
    /// Module preparation requests answered, including every kind of reuse.
    pub compiles: u64,
    /// Instances started on the host.
    pub instances: u64,
    /// Invocations a record answered instead.
    pub answered: u64,
    /// Compiled-module cache accounting and preparation time, absent in older recordings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation: Option<Compilation>,
    /// Elapsed host time of fresh invocations, absent where execution was not timed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ns: Option<u64>,
    /// Module preparation attempts that failed, absent in older recordings and where none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failures: Option<u64>,
}

/// What actual requests of one byte/configuration preparation identity cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleWork {
    /// SHA-256 of the original module bytes.
    pub module: String,
    /// Semantic engine configuration, including host identity and pinned Wasmtime version.
    pub configuration: String,
    /// All actual requests, including refused validation and preparation.
    pub requests: u64,
    /// Actual safe `Module::new` calls, including failed calls.
    pub attempts: u64,
    /// Successful cold preparations.
    pub cold: u64,
    /// Successful preparations loaded by Wasmtime's disk cache.
    pub disk: u64,
    /// Requests answered by the process's compatible prepared module.
    pub process: u64,
    /// Refused requests, including those refused before an attempt.
    pub failures: u64,
    /// Actual cold attempts that failed.
    pub failed_cold: u64,
    /// Actual disk-load attempts that failed.
    pub failed_disk: u64,
    /// Measured time of physical preparations; process reuse contributes no preparation time.
    pub duration_ns: u64,
}

/// The observed answer of one request, including whether a refused request actually attempted work.
#[derive(Debug, Clone, Copy)]
enum ModuleAnswer {
    Prepared(Duration, Reuse),
    Refused(Option<(Duration, bool)>),
}

impl ModuleWork {
    fn new(module: &SealedDigest, configuration: &SealedDigest) -> Self {
        Self {
            module: module.to_string(),
            configuration: configuration.to_string(),
            requests: 0,
            attempts: 0,
            cold: 0,
            disk: 0,
            process: 0,
            failures: 0,
            failed_cold: 0,
            failed_disk: 0,
            duration_ns: 0,
        }
    }

    fn observe(&mut self, answer: ModuleAnswer) -> Result<(), crate::SealedError> {
        add(&mut self.requests, 1)?;
        let duration = match answer {
            ModuleAnswer::Prepared(duration, reuse) => {
                match reuse {
                    Reuse::Cold => {
                        add(&mut self.attempts, 1)?;
                        add(&mut self.cold, 1)?;
                    }
                    Reuse::Disk => {
                        add(&mut self.attempts, 1)?;
                        add(&mut self.disk, 1)?;
                    }
                    Reuse::Process => add(&mut self.process, 1)?,
                }
                duration
            }
            ModuleAnswer::Refused(attempt) => {
                add(&mut self.failures, 1)?;
                match attempt {
                    Some((duration, disk)) => {
                        add(&mut self.attempts, 1)?;
                        if disk {
                            add(&mut self.failed_disk, 1)?;
                        } else {
                            add(&mut self.failed_cold, 1)?;
                        }
                        duration
                    }
                    None => Duration::ZERO,
                }
            }
        };
        let nanos = u64::try_from(duration.as_nanos()).map_err(|_overflow| width())?;
        add(&mut self.duration_ns, nanos)
    }
}

const fn width() -> crate::SealedError {
    crate::SealedError::HostInvariant {
        invariant: crate::error::Invariant::Width,
    }
}

fn add(value: &mut u64, amount: u64) -> Result<(), crate::SealedError> {
    *value = value.checked_add(amount).ok_or_else(width)?;
    Ok(())
}

/// The compiled-module cache's hits and misses, and the elapsed time spent preparing modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compilation {
    /// Requests answered by compiled code already held, on Wasmtime's cache or by this process.
    pub hits: u64,
    /// Requests that performed an actual preparation because no held code answered.
    pub misses: u64,
    /// Nanoseconds spent validating, loading or compiling, and linking modules: the actual preparations, never a reused answer.
    pub duration_ns: u64,
    /// Of the hits, the ones this process's own held module answered, absent in older recordings and where none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<u64>,
    /// Actual `Module::new` preparation attempts, including ones that then failed, absent in older recordings and where none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u64>,
    /// Actual cold preparation attempts that failed, absent in older recordings and where none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_cold: Option<u64>,
    /// Actual disk-load preparation attempts that failed, absent in older recordings and where none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_disk: Option<u64>,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::{Counted, Reuse, counted, elapsed};
    use crate::{SealedDigest, SealedError};

    #[test]
    fn exhausted_atomic_counts_and_elapsed_time_refuse_without_wrapping() {
        let count = AtomicU64::new(u64::MAX);
        assert!(matches!(
            counted(&count),
            Err(SealedError::HostInvariant { .. })
        ));
        assert_eq!(count.load(Ordering::Relaxed), u64::MAX);
        let nanos = AtomicU64::new(u64::MAX - 1);
        assert!(matches!(
            elapsed(&nanos, Duration::from_nanos(2)),
            Err(SealedError::HostInvariant { .. })
        ));
        assert_eq!(nanos.load(Ordering::Relaxed), u64::MAX - 1);
        assert!(elapsed(&nanos, Duration::MAX).is_err());
        assert_eq!(nanos.load(Ordering::Relaxed), u64::MAX - 1);
    }

    #[test]
    fn concurrent_atomic_increments_preserve_every_observed_request() {
        let count = AtomicU64::new(0);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_worker| {
                    let count = &count;
                    njutest_devkit::thread::ScopedThread::launch(scope, move || {
                        for _request in 0..128 {
                            counted(count).expect("the actual count fits");
                        }
                    })
                })
                .collect();
            for worker in workers {
                worker.join().expect("the counting thread is joined");
            }
        });
        assert_eq!(count.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn validation_refusals_have_requests_and_no_invented_attempt() {
        let counted = Counted::default();
        counted.assembled();
        counted
            .failed_module(
                &SealedDigest::of(b"refused"),
                &SealedDigest::of(b"config"),
                None,
            )
            .expect("the observed refusal fits");
        let spent = counted.spent().expect("the bench assembled");
        let modules = spent.modules.expect("the refusal was observed");
        let refused = modules
            .values()
            .next()
            .expect("the refused physical identity");
        assert_eq!(
            (
                refused.requests,
                refused.attempts,
                refused.failures,
                refused.duration_ns
            ),
            (1, 0, 1, 0)
        );
        assert_eq!(spent.compilation, None);
        assert_eq!(spent.compiles, 0);
    }

    #[test]
    fn a_keyed_width_failure_keeps_the_previous_whole_observation() {
        let counted = Counted::default();
        counted.assembled();
        let module = SealedDigest::of(b"module");
        let configuration = SealedDigest::of(b"config");
        counted
            .prepared_module(
                (&module, &configuration),
                Duration::from_nanos(u64::MAX),
                Reuse::Cold,
            )
            .expect("the first measurement fits exactly");
        let before = counted.spent().expect("the actual measurement");
        assert!(matches!(
            counted.prepared_module(
                (&module, &configuration),
                Duration::from_nanos(1),
                Reuse::Cold
            ),
            Err(SealedError::HostInvariant { .. })
        ));
        assert_eq!(counted.spent(), Some(before));
    }
}
