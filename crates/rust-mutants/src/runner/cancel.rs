// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cancellation flags with owned subscriptions and sealed interrupts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvError, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::Clock;
use rust_mutants_sealed::{Interrupt, Raised};

/// A cooperative cancellation flag shared between the caller and a run.
#[derive(Debug, Clone)]
pub struct Cancel {
    own: Arc<Flag>,
    above: Vec<Arc<Flag>>,
    pub(super) clock: Clock,
}

#[derive(Debug, Default)]
struct Flag {
    raised: Arc<Raised>,
    exposed: AtomicBool,
    observers: Mutex<Vec<Weak<Observer>>>,
}

#[derive(Debug)]
struct Observer {
    sent: SyncSender<()>,
    published: AtomicBool,
}

impl Observer {
    fn publish(&self) -> bool {
        if self.published.swap(true, Ordering::AcqRel) {
            return true;
        }
        match self.sent.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => true,
            Err(TrySendError::Disconnected(())) => false,
        }
    }
}

impl Flag {
    fn raise(&self) {
        self.raised.raise();
        let Ok(mut observers) = self.observers.lock() else {
            eprintln!("the cancellation subscription owner was poisoned before publication");
            std::process::abort()
        };
        observers.retain(|observer| match observer.upgrade() {
            Some(observer) => observer.publish(),
            None => false,
        });
    }

    fn watch(&self, observer: &Arc<Observer>) {
        let Ok(mut observers) = self.observers.lock() else {
            eprintln!("the cancellation subscription owner was poisoned before registration");
            std::process::abort()
        };
        observers.retain(|observer| observer.strong_count() != 0);
        observers.push(Arc::downgrade(observer));
        if self.raised.raised() && !observer.publish() {
            let closed = Arc::downgrade(observer);
            observers.retain(|observer| !Weak::ptr_eq(observer, &closed));
        }
    }

    fn flag(&self) -> Arc<AtomicBool> {
        self.exposed.store(true, Ordering::SeqCst);
        self.raised.flag()
    }
}

/// An owned subscription to cancellation, including cancellation of its ancestors.
#[derive(Debug)]
pub struct Cancelled {
    received: Receiver<()>,
    _observer: Arc<Observer>,
}

impl Cancelled {
    /// Waits for the cancellation event, including one published before subscribing.
    ///
    /// # Errors
    /// The subscription producer disconnected.
    pub fn wait(&self) -> Result<(), RecvError> {
        self.received.recv()
    }

    /// Waits for cancellation until the caller's semantic deadline.
    ///
    /// # Errors
    /// The deadline expired or the subscription producer disconnected.
    pub fn wait_timeout(&self, bound: Duration) -> Result<(), RecvTimeoutError> {
        self.received.recv_timeout(bound)
    }
}

impl Cancel {
    /// A flag that is not yet cancelled.
    #[must_use]
    #[expect(
        clippy::new_without_default,
        reason = "execution control is constructed explicitly"
    )]
    pub fn new() -> Self {
        Self {
            own: Arc::new(Flag::default()),
            above: Vec::new(),
            clock: Clock::wall(),
        }
    }

    /// A fresh child cancelled when this flag or any ancestor is cancelled.
    #[must_use]
    pub fn child(&self) -> Self {
        let mut above = self.above.clone();
        above.push(Arc::clone(&self.own));
        Self {
            own: Arc::new(Flag::default()),
            above,
            clock: self.clock.clone(),
        }
    }

    /// Uses the injected supervision clock for this flag and its children.
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Requests cancellation and publishes its one monotonic event to every active subscription.
    pub fn cancel(&self) {
        self.own.raise();
    }

    /// Whether this flag or any ancestor was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.own.raised.raised() || self.above.iter().any(|flag| flag.raised.raised())
    }

    /// Every raw flag a signal handler may store to, own flag first.
    #[must_use]
    pub fn flags(&self) -> Vec<Arc<AtomicBool>> {
        std::iter::once(&self.own)
            .chain(&self.above)
            .map(|flag| flag.flag())
            .collect()
    }

    /// The raw flag the composition root may register with a signal handler.
    #[must_use]
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.own.flag()
    }

    pub(super) fn raw(&self) -> bool {
        std::iter::once(&self.own)
            .chain(&self.above)
            .any(|flag| flag.exposed.load(Ordering::SeqCst))
    }

    /// Subscribes before starting work so cancellation cannot be missed.
    #[must_use]
    pub fn subscribe(&self) -> Cancelled {
        let (sent, received) = mpsc::sync_channel(1);
        let observer = Arc::new(Observer {
            sent,
            published: AtomicBool::new(false),
        });
        for flag in std::iter::once(&self.own).chain(&self.above) {
            flag.watch(&observer);
        }
        Cancelled {
            received,
            _observer: observer,
        }
    }

    /// The sealed interrupt raised by this cancellation and its ancestors.
    #[must_use]
    pub fn interrupt(&self) -> Interrupt {
        let flags = || std::iter::once(&self.own).chain(&self.above);
        Interrupt::raising(flags().map(|flag| Arc::clone(&flag.raised)).collect()).with_raw(
            flags()
                .filter(|flag| flag.exposed.load(Ordering::SeqCst))
                .map(|flag| flag.raised.flag())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Cancel;
    use std::time::Duration;

    #[test]
    fn cancellation_publishes_an_event_to_an_existing_child_subscription() {
        let parent = Cancel::new();
        let child = parent.child();
        let subscription = child.subscribe();
        parent.cancel();
        assert!(child.is_cancelled());
        assert_eq!(subscription.wait_timeout(Duration::ZERO), Ok(()));
    }

    #[test]
    fn a_late_subscription_observes_an_already_published_cancellation() {
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(cancel.subscribe().wait_timeout(Duration::ZERO), Ok(()));
    }

    #[test]
    fn a_subscription_observes_one_monotonic_cancellation_event() {
        let cancel = Cancel::new();
        let subscription = cancel.subscribe();
        cancel.cancel();
        cancel.cancel();
        assert_eq!(subscription.wait_timeout(Duration::ZERO), Ok(()));
        cancel.cancel();
        assert_eq!(
            subscription.wait_timeout(Duration::ZERO),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn one_subscription_unifies_parent_and_child_cancellation() {
        let parent = Cancel::new();
        let child = parent.child();
        let subscription = child.subscribe();
        child.cancel();
        parent.cancel();
        assert_eq!(subscription.wait_timeout(Duration::ZERO), Ok(()));
        assert_eq!(
            subscription.wait_timeout(Duration::ZERO),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn a_closed_subscription_is_pruned_without_losing_a_late_event() {
        let cancel = Cancel::new();
        let subscription = cancel.subscribe();
        drop(subscription);
        cancel.cancel();
        assert!(
            cancel
                .own
                .observers
                .lock()
                .expect("the subscription registry")
                .is_empty()
        );
        assert_eq!(cancel.subscribe().wait_timeout(Duration::ZERO), Ok(()));
    }

    #[test]
    fn a_full_event_slot_preserves_the_existing_cancellation() {
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        sent.try_send(()).expect("the already queued cancellation");
        let observer = super::Observer {
            sent,
            published: std::sync::atomic::AtomicBool::new(false),
        };
        assert!(observer.publish());
        assert_eq!(received.try_recv(), Ok(()));
        assert!(observer.publish());
        assert_eq!(
            received.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        drop(received);
        let observer = super::Observer {
            sent,
            published: std::sync::atomic::AtomicBool::new(false),
        };
        assert!(
            !observer.publish(),
            "a disconnected receiver is explicitly retired"
        );
    }

    #[test]
    fn child_cancellation_does_not_publish_to_its_parent() {
        let parent = Cancel::new();
        let subscription = parent.subscribe();
        parent.child().cancel();
        assert!(!parent.is_cancelled());
        assert_eq!(
            subscription.wait_timeout(Duration::ZERO),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn a_sealed_interrupt_uses_events_until_a_raw_signal_flag_is_exposed() {
        let parent = Cancel::new();
        let child = parent.child();
        assert!(!child.interrupt().raw());
        let raw = parent.flag();
        assert!(child.interrupt().raw());
        raw.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(child.interrupt().raised());
    }

    #[test]
    fn raising_while_subscribing_loses_no_cancellation() {
        for _race in 0..64 {
            let parent = Cancel::new();
            let child = parent.child();
            std::thread::scope(|scope| {
                let raising =
                    njutest_devkit::thread::ScopedThread::launch(scope, || parent.cancel());
                let subscription = child.subscribe();
                raising.join().expect("the cancelling thread joins");
                assert_eq!(subscription.wait_timeout(Duration::ZERO), Ok(()));
            });
        }
    }
}
