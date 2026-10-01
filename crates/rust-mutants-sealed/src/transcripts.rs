// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one sealed invocation established, remembered under the digest of everything it was a function of, so a later invocation of the same module under the same world does not run it again.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Shared counts of modules compiled, instances started, and invocations a record answered.
#[derive(Debug, Clone, Default)]
pub struct Counted {
    inner: Arc<Countings>,
}

/// The counters themselves, behind the shared handle.
#[derive(Debug, Default)]
struct Countings {
    /// Whether any bench of the run assembled, which is what says a run that counted nothing sealed anything at all.
    assembled: AtomicBool,
    /// Modules compiled for the sealed target.
    compiles: AtomicU64,
    compilation_measured: AtomicBool,
    hits: AtomicU64,
    misses: AtomicU64,
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

    /// Says the host compiled one sealed module.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if the count exceeds its recorded width.
    pub fn compiled(&self) -> Result<(), crate::SealedError> {
        counted(&self.inner.compiles)
    }

    /// Records a module preparation, including whether compiled code was reused and its elapsed time.
    ///
    /// # Errors
    /// [`crate::SealedError::HostInvariant`] if a count or elapsed time exceeds its recorded width.
    pub fn prepared(&self, duration: Duration, cached: bool) -> Result<(), crate::SealedError> {
        self.compiled()?;
        if cached {
            counted(&self.inner.hits)?;
        } else {
            counted(&self.inner.misses)?;
        }
        elapsed(&self.inner.compile_ns, duration)?;
        self.inner
            .compilation_measured
            .store(true, Ordering::Relaxed);
        Ok(())
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
        Some(Spent {
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
                }),
            execution_ns: self
                .inner
                .execution_measured
                .load(Ordering::Relaxed)
                .then(|| self.inner.execution_ns.load(Ordering::Relaxed)),
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
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
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
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map(|_previous| ())
        .map_err(|_exhausted| crate::SealedError::HostInvariant {
            invariant: crate::error::Invariant::Width,
        })
}

/// What the host spent on sealed executions, as a recording writes it: the modules compiled, the instances started, and the invocations a record answered instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spent {
    /// Modules compiled for the sealed target.
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
}

/// The compiled-module cache's hits and misses, and the elapsed time spent preparing modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compilation {
    /// Modules loaded from wasmtime's cache.
    pub hits: u64,
    /// Modules compiled because no cached code answered.
    pub misses: u64,
    /// Nanoseconds spent validating, loading or compiling, and linking modules.
    pub duration_ns: u64,
}
