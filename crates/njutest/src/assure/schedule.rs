// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How many mutations a run measures at once, and how the answers come back in the order the catalog has them.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The stack every worker gets, the size a Linux or macOS process's first thread has, so what a worker can measure does not depend on how many workers there are.
pub const WORKER_STACK: usize = 8 << 20;

/// What a worker's item is called in a refusal, so a worker that panicked names what it was measuring.
pub trait Named {
    /// The name.
    fn named(&self) -> String;
}

impl Named for &str {
    fn named(&self) -> String {
        format!("package {self}")
    }
}

impl Named for &rust_mutants::catalog::Mutant {
    fn named(&self) -> String {
        format!("mutation {}", self.display_id)
    }
}

impl Named for u32 {
    fn named(&self) -> String {
        format!("item {self}")
    }
}

impl Named for usize {
    fn named(&self) -> String {
        format!("item {self}")
    }
}

/// How a run starts one of its workers: whether the one at `ordinal` may start, before its thread is asked for.
pub trait Start: Sync {
    /// Nothing, or why the worker at `ordinal` does not start.
    ///
    /// # Errors
    /// Why the worker at `ordinal` does not start.
    fn admit(&self, ordinal: usize) -> std::io::Result<()>;
}

/// Workers started as the operating system starts threads, refused only where it refuses one.
#[derive(Debug, Clone, Copy, Default)]
pub struct Threads;

impl Start for Threads {
    fn admit(&self, _ordinal: usize) -> std::io::Result<()> {
        Ok(())
    }
}

/// Who measures: how many workers, what their threads are called, and how each is started.
#[derive(Debug, Clone, Copy)]
pub struct Crew<S> {
    workers: usize,
    role: &'static str,
    start: S,
}

impl Crew<Threads> {
    /// `workers` threads called `role` and their ordinal, started as the operating system starts threads.
    #[must_use]
    pub const fn threads(workers: usize, role: &'static str) -> Self {
        Self::started(workers, role, Threads)
    }
}

impl<S: Start> Crew<S> {
    /// `workers` threads called `role` and their ordinal, each admitted by `start` first.
    #[must_use]
    pub const fn started(workers: usize, role: &'static str, start: S) -> Self {
        Self {
            workers,
            role,
            start,
        }
    }
}

/// How many mutations to measure at once, given what the configuration asked for, what the machine offers, and whether a resource forces the run to be alone.
#[must_use]
pub fn workers(jobs: rust_mutants::run::Jobs, available: usize, exclusive: bool) -> usize {
    if exclusive {
        return 1;
    }
    jobs.resolve_on(available)
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
    /// The operating system could not report the available processors.
    #[error("the operating system could not report available parallelism: {source}")]
    Parallelism {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// A worker panicked, which is a defect in njutest and meets the same panic every run.
    #[error("{worker} panicked while measuring {item}: {message}")]
    WorkerPanicked {
        /// The worker's thread name.
        worker: String,
        /// What it was measuring.
        item: String,
        /// What the panic said.
        message: String,
    },
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

impl ScheduleError {
    /// The stable code of this refusal: a worker that panicked is a defect to report, and every other one is a run to repeat.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::WorkerPanicked { .. } => crate::error::WORKER_PANICKED,
            Self::Parallelism { .. }
            | Self::WorkerUnstarted { .. }
            | Self::SharedStatePoisoned
            | Self::ExclusiveStatePoisoned
            | Self::ControlStatePoisoned
            | Self::CursorOverflow { .. } => crate::error::SCHEDULER_UNUSABLE,
        }
    }
}

/// What a panic said, where it said it as text.
fn said(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic whose payload is not text".to_owned())
}

/// Marks, while a worker unwinds from `at`, that it panicked there, and moves the cursor past `end` so no other worker takes another item for a measurement that is already a refusal.
struct Measuring<'a> {
    at: usize,
    end: usize,
    next: &'a AtomicUsize,
    panicked_at: &'a AtomicUsize,
}

impl Drop for Measuring<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.next.fetch_max(self.end, Ordering::SeqCst);
            self.panicked_at.store(self.at, Ordering::SeqCst);
        }
    }
}

/// One scoped worker whose normal return is joined explicitly.
/// The lexical scope remains the early-return proof: Rust joins this handle before any borrowed input can leave the scope.
struct ScopedWorker<'scope, T> {
    name: String,
    handle: std::thread::ScopedJoinHandle<'scope, T>,
}

impl<'scope, T: Send + 'scope> ScopedWorker<'scope, T> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        (name, admitted): (String, std::io::Result<()>),
        work: impl FnOnce() -> T + Send + 'scope,
    ) -> Result<Self, ScheduleError> {
        let handle = admitted
            .and_then(|()| {
                std::thread::Builder::new()
                    .name(name.clone())
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, work)
            })
            .map_err(|source| ScheduleError::WorkerUnstarted { source })?;
        Ok(Self { name, handle })
    }

    fn open(&self) {
        self.handle.thread().unpark();
    }

    fn join(self, item: impl FnOnce() -> String) -> Result<T, ScheduleError> {
        self.handle
            .join()
            .map_err(|panic| ScheduleError::WorkerPanicked {
                worker: self.name,
                item: item(),
                message: said(panic.as_ref()),
            })
    }
}

/// Measures every item on the workers of `crew`, never more than there are items, and answers each item in the order the items came in, paired with it.
///
/// No worker takes an item until every worker has started, and none takes another once one has panicked, so a measurement that ends in a refusal stops at once rather than after the whole catalog.
///
/// # Errors
/// Returns [`ScheduleError`] if a worker would not start or panicked, or shared scheduling state can no longer be trusted.
pub fn measure<'a, T, R, F, S>(
    items: &'a [T],
    crew: &Crew<S>,
    work: F,
) -> Result<Vec<(&'a T, R)>, ScheduleError>
where
    T: Sync + Named,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
    S: Start,
{
    let workers = crew.workers.clamp(1, items.len().max(1));
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let cursor = Cursor {
        next: AtomicUsize::new(0),
        opened: AtomicBool::new(false),
        panicked_at: AtomicUsize::new(usize::MAX),
    };
    let mut kept = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let mut unstarted = None;
        for ordinal in 0..workers {
            let (cursor, work) = (&cursor, &work);
            let launched = ScopedWorker::launch(
                scope,
                (
                    format!("{}-{ordinal}", crew.role),
                    crew.start.admit(ordinal),
                ),
                move || cursor.taken(items, work),
            );
            match launched {
                Ok(worker) => handles.push(worker),
                Err(error) => {
                    cursor.next.fetch_max(items.len(), Ordering::SeqCst);
                    unstarted = Some(error);
                    break;
                }
            }
        }
        cursor.opened.store(true, Ordering::SeqCst);
        for worker in &handles {
            worker.open();
        }
        let named = || {
            items
                .get(cursor.panicked_at.load(Ordering::SeqCst))
                .map_or_else(|| "nothing it had taken".to_owned(), Named::named)
        };
        let joined: Vec<_> = handles
            .into_iter()
            .map(|worker| worker.join(named))
            .collect();
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
    kept.sort_by_key(|(at, _, _)| *at);
    Ok(kept
        .into_iter()
        .map(|(_, item, answer)| (item, answer))
        .collect())
}

/// What every worker of one measurement shares: the next item to take, whether they may start taking, and where one panicked.
struct Cursor {
    next: AtomicUsize,
    opened: AtomicBool,
    panicked_at: AtomicUsize,
}

impl Cursor {
    /// Every item one worker takes once the measurement is opened, until none is left, each with its index.
    fn taken<'a, T, R>(
        &self,
        items: &'a [T],
        work: &impl Fn(usize, &T) -> R,
    ) -> Result<Vec<(usize, &'a T, R)>, ScheduleError> {
        while !self.opened.load(Ordering::SeqCst) {
            std::thread::park();
        }
        let mut answered = Vec::new();
        loop {
            let at = match self
                .next
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    value.checked_add(1)
                }) {
                Ok(at) => at,
                Err(at) => return Err(ScheduleError::CursorOverflow { at }),
            };
            let Some(item) = items.get(at) else {
                break;
            };
            let measuring = Measuring {
                at,
                end: items.len(),
                next: &self.next,
                panicked_at: &self.panicked_at,
            };
            let answer = work(at, item);
            drop(measuring);
            answered.push((at, item, answer));
        }
        Ok(answered)
    }
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
