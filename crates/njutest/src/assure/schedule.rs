// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How many mutations a run measures at once, and how the answers come back in the order the catalog has them.

use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The most workers a run gives itself when the configuration does not say.
pub const CAP: usize = 4;

/// The stack every worker gets, the size a process's first thread has, so what a worker can measure does not depend on how many workers there are.
pub const WORKER_STACK: usize = 8 << 20;

/// How many mutations to measure at once, given what the configuration asked for, what the machine offers, and whether a resource forces the run to be alone.
/// # Errors
/// The requested count does not fit this target's address space.
pub fn workers(jobs: u32, available: usize, exclusive: bool) -> Result<usize, ScheduleError> {
    if exclusive {
        return Ok(1);
    }
    if jobs > 0 {
        return usize::try_from(jobs)
            .map_err(|_out_of_range| ScheduleError::WorkerCount { requested: jobs });
    }
    Ok(available.clamp(1, CAP))
}

/// The processors this machine offers.
///
/// # Errors
/// The operating system could not establish the available parallelism.
pub fn available() -> Result<usize, ScheduleError> {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .map_err(|source| ScheduleError::Parallelism { source })
}

/// Why work could not be scheduled without inventing or recovering state.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// The configured worker count does not fit this target.
    #[error("the requested worker count {requested} does not fit this target")]
    WorkerCount {
        /// The count from configuration.
        requested: u32,
    },
    /// The operating system could not report the available processors.
    #[error("the operating system could not report available parallelism: {source}")]
    Parallelism {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// A worker panicked before returning its answers.
    #[error("a worker panicked before returning its answers")]
    WorkerPanicked,
    /// The operating system would not start a worker.
    #[error("the operating system would not start a worker: {source}")]
    WorkerUnstarted {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// A prior panic may have interrupted a shared measurement.
    #[error("a prior panic may have corrupted shared measurement state")]
    SharedStatePoisoned,
    /// A prior panic may have interrupted an exclusive measurement.
    #[error("a prior panic may have corrupted exclusive measurement state")]
    ExclusiveStatePoisoned,
    /// A prior panic may have interrupted the baseline control ledger.
    #[error("a prior panic may have corrupted baseline control state")]
    ControlStatePoisoned,
    /// The shared work cursor reached the end of the address space.
    #[error("the measurement work cursor overflowed at {at}")]
    CursorOverflow {
        /// The first cursor value that could not be advanced.
        at: usize,
    },
}

/// One scoped worker whose normal return is joined explicitly.
/// The lexical scope remains the early-return proof: Rust joins this handle before any borrowed input can leave the scope.
struct ScopedWorker<'scope, T> {
    handle: Option<std::thread::ScopedJoinHandle<'scope, T>>,
}

impl<'scope, T: Send + 'scope> ScopedWorker<'scope, T> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        work: impl FnOnce() -> T + Send + 'scope,
    ) -> Result<Self, ScheduleError> {
        let handle = std::thread::Builder::new()
            .stack_size(WORKER_STACK)
            .spawn_scoped(scope, work)
            .map_err(|source| ScheduleError::WorkerUnstarted { source })?;
        Ok(Self {
            handle: Some(handle),
        })
    }

    fn join(mut self) -> Result<T, ScheduleError> {
        let Some(handle) = self.handle.take() else {
            return Err(ScheduleError::WorkerPanicked);
        };
        match handle.join() {
            Ok(answer) => Ok(answer),
            Err(panic) => {
                drop(panic);
                Err(ScheduleError::WorkerPanicked)
            }
        }
    }
}

/// Measures every item on workers of [`WORKER_STACK`], at most `workers` and never more than there are items, and answers in the order the items came in.
///
/// # Errors
/// Returns [`ScheduleError`] if a worker panics or shared scheduling state can no longer be trusted.
pub fn measure<T, R, F>(items: &[T], workers: usize, work: F) -> Result<Vec<R>, ScheduleError>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    let workers = workers.clamp(1, items.len().max(1));
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let mut kept = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let mut unstarted = None;
        for _ in 0..workers {
            let (next, work) = (&next, &work);
            let launched = ScopedWorker::launch(scope, move || {
                let mut answered = Vec::new();
                loop {
                    let at =
                        match next.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                            value.checked_add(1)
                        }) {
                            Ok(at) => at,
                            Err(at) => return Err(ScheduleError::CursorOverflow { at }),
                        };
                    let Some(item) = items.get(at) else {
                        break;
                    };
                    let answer = work(at, item);
                    answered.push((at, answer));
                }
                Ok(answered)
            });
            match launched {
                Ok(worker) => handles.push(worker),
                Err(error) => {
                    unstarted = Some(error);
                    break;
                }
            }
        }
        let joined: Vec<_> = handles.into_iter().map(ScopedWorker::join).collect();
        if let Some(error) = unstarted {
            return Err(error);
        }
        let mut answered = Vec::with_capacity(items.len());
        for worker in joined {
            let mut from_worker = worker??;
            answered.append(&mut from_worker);
        }
        Ok(answered)
    })?;
    kept.sort_by_key(|(at, _)| *at);
    Ok(kept.into_iter().map(|(_, answer)| answer).collect())
}

/// The machine: shared while a run measures several mutations at once, and given to one of them when a budget expires.
#[derive(Debug, Default)]
pub struct Quiet(RwLock<()>);

impl Quiet {
    /// Runs `work` beside whatever else this run is measuring.
    ///
    /// # Errors
    /// Returns [`ScheduleError`] when an earlier panic poisoned the shared scheduling state.
    pub fn shared<R>(&self, work: impl FnOnce() -> R) -> Result<R, ScheduleError> {
        let held = self
            .0
            .read()
            .map_err(|_poison| ScheduleError::SharedStatePoisoned)?;
        let answer = work();
        drop(held);
        Ok(answer)
    }

    /// Runs `work` with nothing else this run started running beside it.
    ///
    /// # Errors
    /// Returns [`ScheduleError`] when an earlier panic poisoned the exclusive scheduling state.
    pub fn alone<R>(&self, work: impl FnOnce() -> R) -> Result<R, ScheduleError> {
        let held = self
            .0
            .write()
            .map_err(|_poison| ScheduleError::ExclusiveStatePoisoned)?;
        let answer = work();
        drop(held);
        Ok(answer)
    }
}
