// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What stops a guest because whoever runs it stopped, which says nothing about the guest.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::runner::Advances;

/// A flag whose raising is an owned event: a waiter is woken by it, not by looking again.
#[derive(Debug, Default)]
pub struct Raised {
    /// The flag itself, held with whoever raises it from a signal handler.
    flag: Arc<AtomicBool>,
    /// Whether the flag was ever raised, which is what a waiter parks on.
    woken: Mutex<bool>,
    changed: Condvar,
    /// The alarms a raise pings, so an engine's alarm advances its epoch for the stores whose interrupt this flag is part of.
    alarms: Mutex<Vec<Weak<Advances>>>,
}

impl Raised {
    /// A flag that is not yet raised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raises the flag and wakes every waiter and alarm it names.
    /// Idempotent: raising again wakes again, and the flag stays raised.
    pub fn raise(&self) {
        self.flag.store(true, Ordering::SeqCst);
        if let Ok(mut raised) = self.woken.lock() {
            *raised = true;
            drop(raised);
            self.changed.notify_all();
        }
        if let Ok(alarms) = self.alarms.lock() {
            for alarm in alarms.iter() {
                if let Some(alarm) = alarm.upgrade() {
                    alarm.ping();
                }
            }
        }
    }

    /// Whether the flag is raised.
    #[must_use]
    pub fn raised(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// The flag itself, so a composition root can raise it from a signal handler.
    /// A signal handler may store to it and nothing else: it cannot take this type's locks, so a raise that must wake a waiter goes through [`Raised::raise`] on a thread.
    #[must_use]
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }

    /// Waits until the flag is raised or `bound` passes, and says whether it is raised.
    #[must_use]
    pub fn wait_raised(&self, bound: Duration) -> bool {
        let deadline = Instant::now().checked_add(bound);
        let Ok(mut raised) = self.woken.lock() else {
            return self.raised();
        };
        loop {
            if *raised {
                return true;
            }
            let Some(deadline) = deadline else {
                return self.raised();
            };
            let left = deadline.saturating_duration_since(Instant::now());
            let (next, waited) = match self.changed.wait_timeout(raised, left) {
                Ok(waited) => waited,
                Err(_poisoned) => return self.raised(),
            };
            raised = next;
            if waited.timed_out() {
                return self.raised();
            }
        }
    }

    /// Adds `alarm` to the alarms a raise pings, once per alarm.
    pub(crate) fn watch(&self, alarm: &Arc<Advances>) {
        if let Ok(mut alarms) = self.alarms.lock() {
            let watched = Arc::downgrade(alarm);
            if !alarms.iter().any(|alarm| Weak::ptr_eq(alarm, &watched)) {
                alarms.push(watched);
            }
        }
    }
}

/// Flags any one of which, once raised, stops a guest at its next epoch or host call.
#[derive(Debug, Clone)]
pub struct Interrupt {
    flags: Vec<Flag>,
}

/// One flag of an [`Interrupt`], by whether its raising can wake a waiter.
#[derive(Debug, Clone)]
enum Flag {
    /// A flag whose raise pings the alarms that watch it.
    Raised(Arc<Raised>),
    /// A flag a signal handler stores to, whose raising has no wake of its own.
    Raw(Arc<AtomicBool>),
}

impl Interrupt {
    /// An interrupt raised whenever any of `flags` is, whose raising cannot wake a waiter: a signal handler may write these, and a store interrupted by one is delivered at the alarm's typed backstop while such a flag is armed.
    #[must_use]
    pub fn of(flags: Vec<Arc<AtomicBool>>) -> Self {
        Self {
            flags: flags.into_iter().map(Flag::Raw).collect(),
        }
    }

    /// An interrupt raised whenever any of `flags` is raised, which wakes every waiter and alarm watching it.
    #[must_use]
    pub fn raising(flags: Vec<Arc<Raised>>) -> Self {
        Self {
            flags: flags.into_iter().map(Flag::Raised).collect(),
        }
    }

    /// Adds signal-handler flags whose stores require the observed raw-flag backstop.
    #[must_use]
    pub fn with_raw(mut self, flags: Vec<Arc<AtomicBool>>) -> Self {
        self.flags.extend(flags.into_iter().map(Flag::Raw));
        self
    }

    /// Whether any of its flags is raised.
    #[must_use]
    pub fn raised(&self) -> bool {
        self.flags.iter().any(|flag| match flag {
            Flag::Raised(raised) => raised.raised(),
            Flag::Raw(raw) => raw.load(Ordering::SeqCst),
        })
    }

    /// Whether any flag has no wake of its own, which is what asks an alarm for its typed backstop.
    #[must_use]
    pub fn raw(&self) -> bool {
        self.flags.iter().any(|flag| matches!(flag, Flag::Raw(_)))
    }

    /// Adds `alarm` to the alarms every raising flag pings, once per alarm.
    pub(crate) fn watch(&self, alarm: &Arc<Advances>) {
        for flag in &self.flags {
            if let Flag::Raised(raised) = flag {
                raised.watch(alarm);
            }
        }
    }
}
