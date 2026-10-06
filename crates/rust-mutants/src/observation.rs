// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Producer-owned host observations with retained subscriptions and measured semantic deadlines.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use notify::Watcher as _;
use serde::Serialize;

/// The bounded backlog, whose overflow remains a sticky refusal for the entire subscription.
const BACKLOG: usize = 64;

/// The awaited marker index of a native resource stream on which no fence is waiting.
const UNFENCED: usize = usize::MAX;

/// The native macOS resource stream, without one descriptor for every observed file.
#[cfg(target_os = "macos")]
type ResourceWatcher = notify::FsEventWatcher;

/// The native resource subscription on the other supported hosts.
#[cfg(not(target_os = "macos"))]
type ResourceWatcher = notify::RecommendedWatcher;

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
    reader: Arc<Mutex<Thread>>,
    lost: Arc<Mutex<Option<io::Error>>>,
    invalidated: Arc<AtomicBool>,
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
        self.wake();
    }

    fn wake(&self) {
        match self.reader.lock() {
            Ok(reader) => reader.unpark(),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
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

/// A coalesced resource wake whose pending change survives until this subscription receives it.
#[derive(Debug, Clone)]
pub struct Invalidation {
    signal: Signal,
}

impl Invalidation {
    /// Retains one pending resource change without enqueueing a counted product event.
    pub fn changed(&self) {
        self.signal.invalidated.store(true, Ordering::Release);
        self.signal.wake();
    }

    /// Retains the first producer refusal independently of the pending resource change.
    pub fn failed(&self, error: io::Error) {
        self.signal.failed(error);
    }
}

/// The excluded generated paths of one native resource stream and the marker its reader awaits.
#[derive(Debug, Clone)]
struct Fence {
    root: PathBuf,
    generated: Arc<[PathBuf]>,
    awaited: Arc<AtomicUsize>,
}

impl Fence {
    /// Wakes the reader without a resource change once the stream delivers its awaited marker.
    fn arrived(&self, paths: &[PathBuf], reader: &Invalidation) {
        let awaited = self.awaited.load(Ordering::Acquire);
        if self
            .generated
            .get(awaited)
            .is_some_and(|marker| paths.contains(marker))
            && self
                .awaited
                .compare_exchange(awaited, UNFENCED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            reader.signal.wake();
        }
    }
}

/// An owned subscription registered before a producer starts or an initial resource observation is made.
pub struct Observation {
    received: Receiver<io::Result<Event>>,
    signal: Arc<Signal>,
    reader: thread::ThreadId,
    watcher: Option<ResourceWatcher>,
    fence: Option<Fence>,
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
            signal: Arc::new(Signal {
                sent,
                reader: Arc::new(Mutex::new(thread::current())),
                lost: Arc::new(Mutex::new(None)),
                invalidated: Arc::new(AtomicBool::new(false)),
            }),
            reader: thread::current().id(),
            watcher: None,
            fence: None,
        }
    }

    /// Subscribes to native filesystem change events before the caller first reads `root`.
    ///
    /// # Errors
    /// The operating system could not subscribe to every requested directory.
    pub fn filesystem(root: &Path, recursive: bool) -> io::Result<Self> {
        Self::filesystem_except(root, recursive, &[])
    }

    /// Subscribes before reading the resource, excluding only the explicitly named generated subtrees.
    ///
    /// # Errors
    /// A requested root or excluded spelling cannot be subscribed exactly.
    pub fn filesystem_except(root: &Path, recursive: bool, excluded: &[&str]) -> io::Result<Self> {
        let mut observed = Self::subscribe();
        let signal = observed.invalidation();
        let root = std::fs::canonicalize(root)?;
        let fence = Fence {
            generated: excluded.iter().map(|name| root.join(name)).collect(),
            root,
            awaited: Arc::new(AtomicUsize::new(UNFENCED)),
        };
        let fenced = fence.clone();
        let mut watcher = ResourceWatcher::new(
            move |event: notify::Result<notify::Event>| match event {
                Ok(event) if event.kind.is_access() => {}
                Ok(event)
                    if !event.paths.is_empty()
                        && event.paths.iter().all(|path| {
                            fenced
                                .generated
                                .iter()
                                .any(|generated| path.starts_with(generated))
                        }) =>
                {
                    fenced.arrived(&event.paths, &signal);
                }
                Ok(_changed) => signal.changed(),
                Err(source) => signal.failed(io::Error::other(source)),
            },
            notify::Config::default(),
        )
        .map_err(io::Error::other)?;
        watcher
            .watch(
                &fence.root,
                if recursive {
                    notify::RecursiveMode::Recursive
                } else {
                    notify::RecursiveMode::NonRecursive
                },
            )
            .map_err(io::Error::other)?;
        observed.watcher = Some(watcher);
        observed.fence = Some(fence);
        Ok(observed)
    }

    /// The endpoint retained by each explicit producer of this subscription.
    #[must_use]
    pub fn signal(&self) -> Signal {
        self.signal.as_ref().clone()
    }

    /// A publication endpoint whose retained lifetime proves only that this reader subscription still exists.
    #[must_use]
    pub fn retained_signal(&self) -> Weak<Signal> {
        Arc::downgrade(&self.signal)
    }

    /// The distinct endpoint for a latest-resource wake rather than a counted event sequence.
    #[must_use]
    pub fn invalidation(&self) -> Invalidation {
        Invalidation {
            signal: self.signal(),
        }
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
            Err(TryRecvError::Empty) => self
                .signal
                .invalidated
                .swap(false, Ordering::AcqRel)
                .then_some(Event::Changed),
            Err(TryRecvError::Disconnected) => {
                return Err(io::Error::other("the observation producers disconnected"));
            }
        };
        self.ensure_complete()?;
        Ok(event)
    }

    /// Acknowledges a wake whose actual data was read independently, retaining every failure.
    ///
    /// # Errors
    /// A producer failed or its observation backlog lost evidence.
    pub fn acknowledge(&self) -> io::Result<()> {
        self.pending().map(|_wake| ())
    }

    /// Creates `marker` and returns once this subscription's own native stream delivers it, after every earlier change.
    ///
    /// # Errors
    /// The subscription has no native stream or another reader, `marker` is not a fresh excluded file name of its root, or a producer failed.
    pub fn fence(&self, marker: &str) -> io::Result<()> {
        let Some(fence) = &self.fence else {
            return Err(io::Error::other(
                "only a native resource subscription delivers a fence",
            ));
        };
        if thread::current().id() != self.reader {
            return Err(io::Error::other(
                "a fence is awaited on the subscribed thread its stream wakes",
            ));
        }
        let path = fence.root.join(marker);
        let awaited = fence
            .generated
            .iter()
            .position(|generated| *generated == path)
            .filter(|_excluded| Path::new(marker).file_name() == Some(OsStr::new(marker)))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("the fence marker {marker:?} is not an excluded file name"),
                )
            })?;
        fence.awaited.store(awaited, Ordering::Release);
        let fenced = self.fenced(fence, &path, awaited);
        fence.awaited.store(UNFENCED, Ordering::Release);
        fenced
    }

    fn fenced(&self, fence: &Fence, marker: &Path, awaited: usize) -> io::Result<()> {
        let created = std::fs::File::create_new(marker)?;
        drop(created);
        while fence.awaited.load(Ordering::Acquire) == awaited {
            self.ensure_complete()?;
            thread::park();
        }
        self.ensure_complete()
    }

    /// Transfers this exclusive reader while retaining every already registered producer and queued event.
    ///
    /// # Errors
    /// The reader ownership mutex is poisoned.
    pub fn bind_current_thread(&mut self) -> io::Result<()> {
        let mut reader =
            self.signal.reader.lock().map_err(|_poisoned| {
                io::Error::other("the observation reader mutex is poisoned")
            })?;
        *reader = thread::current();
        self.reader = reader.id();
        drop(reader);
        Ok(())
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
        if thread::current().id() != self.reader || owner.is_empty() || cause.is_empty() {
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
            match self.pending() {
                Ok(Some(event)) => break Ok(event),
                Err(failure) => break Err(failure),
                Ok(None) => {}
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

    /// Retains B's cancellation subscription and an owned bridge for this observation's lifetime.
    ///
    /// # Errors
    /// The bridge thread could not be started.
    pub fn cancellation(&self, cancel: &crate::runner::Cancel) -> io::Result<Cancellation> {
        Ok(Cancellation {
            worker: CancellationThread::launch(cancel, self.signal())?,
        })
    }
}

/// A cancellation bridge whose subscription and worker end before its owner is disposed.
#[derive(Debug)]
pub struct Cancellation {
    worker: CancellationThread,
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        self.worker.finish();
    }
}

#[derive(Debug)]
struct CancellationThread {
    handle: Option<thread::JoinHandle<()>>,
    stop: crate::runner::Cancel,
}

impl CancellationThread {
    fn launch(cancel: &crate::runner::Cancel, signal: Signal) -> io::Result<Self> {
        let stop = cancel.child();
        let subscription = stop.subscribe();
        let handle = thread::Builder::new()
            .name("cancellation-observation".to_owned())
            .spawn(move || match subscription.wait() {
                Ok(()) => signal.publish(Event::Cancelled),
                Err(source) => signal.failed(io::Error::other(source)),
            })?;
        Ok(Self {
            handle: Some(handle),
            stop,
        })
    }

    fn finish(&mut self) {
        self.stop.cancel();
        if let Some(handle) = self.handle.take()
            && let Err(panic) = handle.join()
        {
            drop(panic);
            std::process::abort();
        }
    }
}

impl Drop for CancellationThread {
    fn drop(&mut self) {
        self.finish();
    }
}
