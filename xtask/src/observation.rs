// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Producer-owned host observations with retained subscriptions and measured semantic deadlines.

use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use notify::Watcher as _;
use serde::Serialize;

/// The bounded backlog, whose overflow remains a sticky refusal for the entire subscription.
const BACKLOG: usize = 64;

/// The semantic deadline clock, independent of the executing host's wait measurement.
pub trait Clock {
    /// The current monotonic point used to decide the semantic deadline.
    fn now(&self) -> Instant;
    /// Blocks on a producer wake or this one remaining semantic duration.
    ///
    /// # Errors
    /// The clock could not perform its declared wait.
    fn park(&self, remaining: Option<Duration>) -> io::Result<()>;
}

/// The executing host clock, blocking only on an explicit publication or semantic deadline.
#[derive(Debug, Clone, Copy)]
pub struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn park(&self, remaining: Option<Duration>) -> io::Result<()> {
        match remaining {
            Some(remaining) => thread::park_timeout(remaining),
            None => thread::park(),
        }
        Ok(())
    }
}

/// One named producer observation and its optional semantic deadline.
#[derive(Debug, Clone, Copy)]
pub struct Waiting<'a> {
    /// The actual producer or resource.
    pub owner: &'a str,
    /// The event or semantic deadline being awaited.
    pub cause: &'a str,
    /// The semantic endpoint, never a retry interval.
    pub deadline: Option<Instant>,
}

/// An observation published by its producer, or the observer's semantic deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The subscribed resource changed.
    Changed,
    /// The producer finished.
    Completed,
    /// The subscribed cancellation was raised.
    Cancelled,
    /// The caller's semantic deadline elapsed.
    Deadline,
}

/// The executing host on which a measured wait took place.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Machine {
    /// The operating system that executed the wait.
    pub os: &'static str,
    /// The positive processor count observed on that host.
    pub cpus: usize,
}

/// A measured host-wait note, kept apart from the actual observation result.
#[derive(Debug, Clone, Serialize)]
pub struct WaitNote {
    /// The concrete producer or resource identity being awaited.
    pub owner: String,
    /// The producer event or semantic deadline being awaited.
    pub cause: String,
    /// The measured monotonic duration of this wait.
    pub elapsed_ns: u64,
    /// The host that executed the wait.
    pub machine: Machine,
}

/// The actual observation and the measured wait that received it.
#[derive(Debug)]
pub struct Waited {
    /// The event received or deadline reached.
    pub event: io::Result<Event>,
    /// The independently measured host wait.
    pub note: WaitNote,
}

/// A producer's publication endpoint, waking the subscribed reader after enqueueing.
#[derive(Debug, Clone)]
pub struct Signal {
    sent: SyncSender<io::Result<Event>>,
    reader: Thread,
    lost: Arc<Mutex<Option<io::Error>>>,
}

impl Signal {
    /// Publishes `event` before waking the reader, including a publication made before its wait.
    pub fn publish(&self, event: Event) {
        self.deliver(Ok(event));
    }

    /// Publishes the producer's observation failure rather than treating it as no change.
    pub fn failed(&self, error: io::Error) {
        self.deliver(Err(error));
    }

    fn deliver(&self, event: io::Result<Event>) {
        let mut lost = match self.lost.lock() {
            Ok(lost) => lost,
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        };
        if let Err(source) = &event
            && lost.is_none()
        {
            *lost = Some(io::Error::new(source.kind(), source.to_string()));
        }
        match self.sent.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                if lost.is_none() {
                    *lost = Some(io::Error::other(format!(
                        "the bounded observation backlog is full: {event:?}"
                    )));
                }
            }
            Err(TrySendError::Disconnected(_reader_has_ended)) => {}
        }
        drop(lost);
        self.reader.unpark();
    }

    fn failure(&self) -> io::Result<Option<io::Error>> {
        let lost = self
            .lost
            .lock()
            .map_err(|_poisoned| io::Error::other("the observation failure mutex is poisoned"))?;
        let failure = lost
            .as_ref()
            .map(|source| io::Error::new(source.kind(), source.to_string()));
        drop(lost);
        Ok(failure)
    }
}

/// An owned subscription registered before a producer starts or an initial resource observation is made.
pub struct Observation {
    received: Receiver<io::Result<Event>>,
    signal: Signal,
    watcher: Option<notify::RecommendedWatcher>,
}

impl std::fmt::Debug for Observation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Observation")
            .field("signal", &self.signal)
            .field("filesystem", &self.watcher.is_some())
            .finish_non_exhaustive()
    }
}

impl Observation {
    /// Subscribes the current thread before the returned producer endpoint is used.
    #[must_use]
    pub fn subscribe() -> Self {
        let (sent, received) = mpsc::sync_channel(BACKLOG);
        Self {
            received,
            signal: Signal {
                sent,
                reader: thread::current(),
                lost: Arc::new(Mutex::new(None)),
            },
            watcher: None,
        }
    }

    /// Subscribes to native filesystem change events before the caller first reads `root`.
    ///
    /// # Errors
    /// The operating system could not subscribe to every requested directory.
    pub fn filesystem(root: &Path, recursive: bool) -> io::Result<Self> {
        let mut observed = Self::subscribe();
        let signal = observed.signal();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if event.kind.is_access() => {}
                Ok(_changed) => signal.publish(Event::Changed),
                Err(source) => signal.failed(io::Error::other(source)),
            })
            .map_err(io::Error::other)?;
        watcher
            .watch(
                root,
                if recursive {
                    notify::RecursiveMode::Recursive
                } else {
                    notify::RecursiveMode::NonRecursive
                },
            )
            .map_err(io::Error::other)?;
        observed.watcher = Some(watcher);
        Ok(observed)
    }

    /// The endpoint retained by each explicit producer of this subscription.
    #[must_use]
    pub fn signal(&self) -> Signal {
        self.signal.clone()
    }

    /// Refuses every queued-result decision after a retained producer or backlog failure.
    ///
    /// # Errors
    /// The subscription lost evidence or a producer retained a failure.
    pub fn ensure_complete(&self) -> io::Result<()> {
        match self.signal.failure()? {
            Some(source) => Err(source),
            None => Ok(()),
        }
    }

    /// Consumes one retained wake after reading the actual data, preserving every sticky refusal.
    ///
    /// # Errors
    /// The subscription lost evidence or a producer retained a failure.
    pub fn pending(&self) -> io::Result<Option<Event>> {
        self.ensure_complete()?;
        let event = match self.received.try_recv() {
            Ok(event) => Some(event?),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                return Err(io::Error::other("the observation producers disconnected"));
            }
        };
        self.ensure_complete()?;
        Ok(event)
    }

    /// Waits on an explicit publication or the one semantic `deadline`, never a sampling interval.
    ///
    /// # Errors
    /// A producer failed, the wait moved to an unsubscribed thread, or its measurement cannot be represented.
    pub fn wait(&self, owner: &str, cause: &str, deadline: Option<Instant>) -> io::Result<Waited> {
        self.wait_with(
            Waiting {
                owner,
                cause,
                deadline,
            },
            &WallClock,
        )
    }

    /// Waits with an injected semantic clock while measuring the executing host independently.
    ///
    /// # Errors
    /// A producer failed, the caller moved threads, or its measurement cannot be represented.
    pub fn wait_with(&self, waiting: Waiting<'_>, clock: &impl Clock) -> io::Result<Waited> {
        let Waiting {
            owner,
            cause,
            deadline,
        } = waiting;
        if thread::current().id() != self.signal.reader.id() || owner.is_empty() || cause.is_empty()
        {
            return Err(io::Error::other(
                "a host wait must name its producer and event on the subscribed thread",
            ));
        }
        let machine = Machine {
            os: std::env::consts::OS,
            cpus: thread::available_parallelism()?.get(),
        };
        let started = Instant::now();
        let event = loop {
            match self.signal.failure() {
                Ok(Some(failure)) | Err(failure) => break Err(failure),
                Ok(None) => {}
            }
            match self.received.try_recv() {
                Ok(event) => break event,
                Err(TryRecvError::Disconnected) => {
                    break Err(io::Error::other("the observation producers disconnected"));
                }
                Err(TryRecvError::Empty) => {}
            }
            match deadline {
                Some(deadline) => match deadline.checked_duration_since(clock.now()) {
                    Some(left) if !left.is_zero() => {
                        if let Err(source) = clock.park(Some(left)) {
                            break Err(source);
                        }
                    }
                    Some(_) | None => break Ok(Event::Deadline),
                },
                None => {
                    if let Err(source) = clock.park(None) {
                        break Err(source);
                    }
                }
            }
        };
        let elapsed_ns = u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?;
        let event = match self.signal.failure() {
            Ok(Some(failure)) | Err(failure) => Err(failure),
            Ok(None) => event,
        };
        Ok(Waited {
            event,
            note: WaitNote {
                owner: owner.to_owned(),
                cause: cause.to_owned(),
                elapsed_ns,
                machine,
            },
        })
    }
}
