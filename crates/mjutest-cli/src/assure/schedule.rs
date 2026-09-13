// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How many mutations a run measures at once, and how the answers come back in the order the catalog has them.
//!
//! Measuring two mutations at once is not a budget: every mutation still runs,
//! against every test its route named, and nothing is sampled, skipped, or
//! predicted. What changes is only which processes overlap in time, which is
//! why the worker count is outside every behaviour key
//! ([the assurance contract](../../../../docs/assurance-contract.md)).
//!
//! What must not change is the answer. Workers measure in whatever order they
//! get to an item and commit nothing; this module puts the answers back in the
//! order the items came in, so a report's bytes are the same however the
//! processors were shared out.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

/// The most workers a run gives itself when the configuration does not say.
///
/// A run measures a test suite, and a suite is entitled to the machine it is
/// measured on: helping itself to every processor makes the timings it
/// calibrates against its own contention.
pub const CAP: usize = 4;

/// How many mutations to measure at once, given what the configuration asked for, what the machine offers, and whether a resource forces the run to be alone.
#[must_use]
pub fn workers(jobs: u32, available: usize, exclusive: bool) -> usize {
    if exclusive {
        return 1;
    }
    if jobs > 0 {
        return usize::try_from(jobs).unwrap_or(usize::MAX);
    }
    available.clamp(1, CAP)
}

/// The processors this machine offers, or one when it will not say.
#[must_use]
pub fn available() -> usize {
    std::thread::available_parallelism()
        .map_or(std::num::NonZeroUsize::MIN.get(), std::num::NonZero::get)
}

/// Measures every item, at most `workers` at a time, and answers in the order the items came in.
///
/// One worker, or one item, is measured on this thread: a run that cannot
/// overlap anything should not pay for threads that would not overlap.
pub fn measure<T, R, F>(items: &[T], workers: usize, work: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    if workers <= 1 || items.len() <= 1 {
        return items
            .iter()
            .enumerate()
            .map(|(at, item)| work(at, item))
            .collect();
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (next, done, work) = (&next, &done, &work);
            drop(scope.spawn(move || {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(at) else {
                        break;
                    };
                    let answer = work(at, item);
                    let mut kept = done.lock().unwrap_or_else(PoisonError::into_inner);
                    kept.push((at, answer));
                }
            }));
        }
    });
    let mut kept = done.into_inner().unwrap_or_else(PoisonError::into_inner);
    kept.sort_by_key(|(at, _)| *at);
    kept.into_iter().map(|(_, answer)| answer).collect()
}

/// The machine: shared while a run measures several mutations at once, and given to one of them when a budget expires.
///
/// A mutation's budget is five times a duration the baseline measured, and a
/// duration measured while three other test processes were running is a fact
/// about the load rather than about the mutation. A run that has to decide
/// whether a budget really expired takes the machine to itself first, so that
/// the measurement the decision rests on is the one the budget was calibrated
/// for. It is not a retry policy: one expired budget buys one quiet
/// measurement, and what that measurement observes is what stands.
#[derive(Debug, Default)]
pub struct Quiet(RwLock<()>);

impl Quiet {
    /// Runs `work` beside whatever else this run is measuring.
    pub fn shared<R>(&self, work: impl FnOnce() -> R) -> R {
        let _held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        work()
    }

    /// Runs `work` with nothing else this run started running beside it.
    pub fn alone<R>(&self, work: impl FnOnce() -> R) -> R {
        let _held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        work()
    }
}
