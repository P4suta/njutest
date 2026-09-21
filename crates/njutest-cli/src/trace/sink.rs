// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a recording goes: memory, a writer, a directory, or several at once.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::event::{Event, Payload};

/// The JSON Lines stream of a directory sink, one event per line.
pub const FILE_NAME: &str = "trace.jsonl";

/// The subdirectory of a directory sink that holds preserved output.
pub const OUTPUT_DIRECTORY_NAME: &str = "output";

/// Caps one preserved output file. A capture larger than the limit is cut to it; the event still digests the whole capture.
pub(super) const OUTPUT_FILE_LIMIT: usize = 1 << 20;

/// Ends a preserved output file that did not fit.
pub(super) const TRUNCATION_MARKER: &str = "...";

/// How many events a run that asked for no trace keeps in memory, so a run that fails still has its last moments to put in a diagnostics bundle.
#[cfg(any(test, feature = "testkit"))]
pub const RING_CAPACITY: usize = 4096;
#[cfg(not(any(test, feature = "testkit")))]
const RING_CAPACITY: usize = 4096;

/// Where a recording goes.
#[derive(Debug)]
pub enum Sink {
    /// The most recent events in memory: the ring an untraced run keeps and the sink a test reads.
    Memory(MemorySink),
    /// A durable directory that must keep every event, observed by an
    /// in-memory ring whose success can never hide a failed durable write.
    Required(RequiredSink),
}

/// One durable trace authority and its non-authoritative diagnostic ring.
#[derive(Debug)]
pub struct RequiredSink {
    authority: DirSink,
    observer: MemorySink,
}

/// Whether the best-effort in-memory observer can account for its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObserverState {
    /// Its internal state is consistent.
    Complete {
        /// Events deliberately evicted by the bounded ring.
        dropped: u64,
    },
    /// Mutex poisoning or counter overflow made its accounting unknowable.
    Corrupt,
}

impl Sink {
    /// The ring an untraced run records into.
    #[must_use]
    pub const fn ring() -> Self {
        Self::Memory(MemorySink::ring())
    }

    /// Makes `authority` the only durable authority while retaining a
    /// best-effort ring for an in-process diagnostic bundle.
    #[must_use]
    pub const fn required_with_ring(authority: DirSink) -> Self {
        Self::Required(RequiredSink {
            authority,
            observer: MemorySink::ring(),
        })
    }

    /// Keeps one event.
    ///
    /// # Errors
    /// The reason the event was not kept. The recorder counts it and moves
    /// on.
    pub fn emit(&self, event: &Event) -> io::Result<()> {
        match self {
            Self::Memory(sink) => sink.emit(event),
            Self::Required(sink) => {
                let durable = sink.authority.emit(event);
                sink.observer.observe(event);
                durable
            }
        }
    }

    /// How many events this sink lost, when it is the authority on that: a full ring absorbs a loss without an error, and only the sink knows. `None` leaves the count to the recorder's observed failures.
    #[must_use]
    pub fn dropped(&self) -> Option<u64> {
        match self {
            Self::Memory(sink) => sink.dropped(),
            Self::Required(sink) => Some(sink.authority.dropped()),
        }
    }

    /// Every event a memory sink anywhere in this sink kept, oldest first. A recording with no memory sink has nothing to answer with.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn events(&self) -> Vec<Event> {
        match self {
            Self::Memory(sink) => sink.events(),
            Self::Required(sink) => sink.observer.events(),
        }
    }

    /// Releases what the sink holds. Called once by the recorder at the end of the recording; closing twice is not an error.
    ///
    /// # Errors
    /// The failure to make the recording durable.
    pub fn close(&self) -> io::Result<()> {
        match self {
            Self::Memory(sink) => {
                sink.close();
                Ok(())
            }
            Self::Required(sink) => {
                let durable = sink.authority.close();
                sink.observer.close();
                durable
            }
        }
    }

    /// Whether this sink represents an explicit durable recording request.
    #[must_use]
    pub(crate) const fn is_required(&self) -> bool {
        matches!(self, Self::Required(_))
    }

    #[cfg(any(test, feature = "testkit"))]
    #[cfg(feature = "testkit")]
    pub(crate) fn fail_durable_writes(&self) {
        match self {
            Self::Required(sink) => sink.authority.fail_writes(),
            Self::Memory(_) => {}
        }
    }
}

/// Keeps the most recent events in memory: the sink of a test, of an in-process reader, and of an untraced run's ring. A full ring drops its oldest event and counts it.
#[derive(Debug)]
pub struct MemorySink {
    capacity: Option<usize>,
    events: Mutex<VecDeque<Event>>,
    dropped: AtomicU64,
    corrupt: AtomicBool,
    closed: AtomicBool,
}

impl MemorySink {
    /// Keeps everything.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn unbounded() -> Self {
        Self::with_capacity(None)
    }

    /// Keeps the newest `capacity` events.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn bounded(capacity: usize) -> Self {
        Self::with_capacity(Some(capacity))
    }

    /// The ring an untraced run records into: the last [`RING_CAPACITY`] events, and nothing on disk until a failure asks for them.
    #[must_use]
    pub const fn ring() -> Self {
        Self::with_capacity(Some(RING_CAPACITY))
    }

    const fn with_capacity(capacity: Option<usize>) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::new()),
            dropped: AtomicU64::new(0),
            corrupt: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }

    /// The kept events, oldest first.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn events(&self) -> Vec<Event> {
        match self.events.lock() {
            Ok(events) => events.iter().cloned().collect(),
            Err(_poisoned) => {
                self.corrupt.store(true, Ordering::SeqCst);
                Vec::new()
            }
        }
    }

    /// Whether [`Sink::close`] was called.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl MemorySink {
    /// Keeps one event, dropping the oldest when the ring is full. Memory does not fail: what a full ring loses it counts.
    fn emit(&self, event: &Event) -> io::Result<()> {
        let evicted = {
            let mut events = self.events.lock().map_err(|_poisoned| {
                self.corrupt.store(true, Ordering::SeqCst);
                io::Error::other("the trace observer mutex is poisoned")
            })?;
            match self.capacity {
                Some(0) => 1,
                Some(capacity) => {
                    if events.len() > capacity {
                        self.corrupt.store(true, Ordering::SeqCst);
                        return Err(io::Error::other(
                            "the trace observer exceeded its fixed capacity",
                        ));
                    }
                    let evicted = if events.len() == capacity {
                        events.pop_front();
                        1
                    } else {
                        0
                    };
                    events.push_back(event.clone());
                    evicted
                }
                None => {
                    events.push_back(event.clone());
                    0
                }
            }
        };
        if evicted != 0
            && let Err(error) = count_drop(&self.dropped)
        {
            self.corrupt.store(true, Ordering::SeqCst);
            return Err(error);
        }
        Ok(())
    }

    /// Offers an event to the non-authoritative diagnostic ring. A loss is
    /// retained as [`ObserverState::Corrupt`] or in its checked drop count,
    /// never promoted into evidence that the durable authority succeeded.
    fn observe(&self, event: &Event) {
        match self.emit(event) {
            Ok(()) => {}
            Err(_loss_recorded_in_observer_state) => {}
        }
    }

    fn state(&self) -> ObserverState {
        if self.corrupt.load(Ordering::SeqCst) {
            ObserverState::Corrupt
        } else {
            ObserverState::Complete {
                dropped: self.dropped.load(Ordering::SeqCst),
            }
        }
    }

    /// How many events the ring evicted, unless its accounting became
    /// unknowable.
    fn dropped(&self) -> Option<u64> {
        match self.state() {
            ObserverState::Complete { dropped } => Some(dropped),
            ObserverState::Corrupt => None,
        }
    }

    /// Nothing to release; the flag is what a test reads.
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// One event as a JSON line, newline included.
fn encode(event: &Event) -> io::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(event).map_err(io::Error::other)?;
    line.push(b'\n');
    Ok(line)
}

/// Writes a recording to a directory: the stream in [`FILE_NAME`] and the output of the commands that produced any under [`OUTPUT_DIRECTORY_NAME`].
#[derive(Debug)]
pub struct DirSink {
    directory: PathBuf,
    stream: Mutex<Option<File>>,
    output_exists: AtomicBool,
    dropped: AtomicU64,
    #[cfg(any(test, feature = "testkit"))]
    fail_writes: AtomicBool,
}

impl DirSink {
    /// Creates `directory` and opens its stream.
    ///
    /// # Errors
    /// The failure to create the directory (including `AlreadyExists`) or to
    /// open the stream. An explicit trace caller treats this as a setup
    /// failure; an unrequested trace uses only its in-memory ring.
    pub fn create(directory: &Path) -> io::Result<Self> {
        if let Some(parent) = directory.parent() {
            fs::create_dir_all(parent)?;
        }
        #[expect(
            clippy::create_dir,
            reason = "exclusive creation is the point: a directory another recording owns is refused"
        )]
        fs::create_dir(directory)?;
        let stream = File::options()
            .create_new(true)
            .write(true)
            .open(directory.join(FILE_NAME))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            stream: Mutex::new(Some(stream)),
            output_exists: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            #[cfg(any(test, feature = "testkit"))]
            fail_writes: AtomicBool::new(false),
        })
    }

    /// The directory of this recording.
    #[cfg(any(test, feature = "testkit"))]
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[cfg(any(test, feature = "testkit"))]
    #[cfg(feature = "testkit")]
    fn fail_writes(&self) {
        self.fail_writes.store(true, Ordering::SeqCst);
    }

    /// Writes the captured output of an exec event beside the stream and returns the event pointing at the file. Best effort: an output that cannot be written costs its path, not the event. The event is copied so a sink sharing it with others never leaks this sink's paths.
    fn preserve_output(&self, event: &Event) -> Option<Event> {
        let Payload::Exec { exec } = &event.payload else {
            return None;
        };
        if exec.output.is_empty() {
            return None;
        }
        let (data, truncated) = limit_output(&exec.output);
        let name = format!("{}.txt", event.seq);
        match self.write_output(&name, &data) {
            Ok(()) => {}
            Err(_output_not_preserved) => return None,
        }
        let mut record = exec.clone();
        record.output_truncated = truncated;
        record.output_path = Some(format!("{OUTPUT_DIRECTORY_NAME}/{name}"));
        record.output.clear();
        Some(Event {
            seq: event.seq,
            timestamp: event.timestamp.clone(),
            elapsed_ms: event.elapsed_ms,
            payload: Payload::Exec { exec: record },
        })
    }

    fn write_output(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let directory = self.directory.join(OUTPUT_DIRECTORY_NAME);
        if !self.output_exists.load(Ordering::SeqCst) {
            match fs::DirBuilder::new().create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&directory)?;
                    if !metadata.file_type().is_dir() {
                        return Err(io::Error::other(
                            "the trace output entry is not a real directory",
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
            self.output_exists.store(true, Ordering::SeqCst);
        }
        let mut output = File::options()
            .create_new(true)
            .write(true)
            .open(directory.join(name))?;
        output.write_all(data)?;
        output.sync_all()
    }
}

/// Caps a capture at [`OUTPUT_FILE_LIMIT`], marking what it cut.
fn limit_output(output: &[u8]) -> (Vec<u8>, bool) {
    match output.get(..OUTPUT_FILE_LIMIT) {
        Some(head) if output.len() > OUTPUT_FILE_LIMIT => {
            let mut limited = Vec::new();
            limited.extend_from_slice(head);
            limited.extend_from_slice(TRUNCATION_MARKER.as_bytes());
            (limited, true)
        }
        _ => (output.to_vec(), false),
    }
}

impl DirSink {
    /// Writes one event, preserving the output of a command that produced any.
    fn emit(&self, event: &Event) -> io::Result<()> {
        #[cfg(any(test, feature = "testkit"))]
        if self.fail_writes.load(Ordering::SeqCst) {
            count_drop(&self.dropped)?;
            return Err(io::Error::other("injected durable trace write failure"));
        }
        let preserved = self.preserve_output(event);
        let recorded = match preserved.as_ref() {
            Some(preserved) => preserved,
            None => event,
        };
        let line = match encode(recorded) {
            Ok(line) => line,
            Err(error) => {
                count_drop(&self.dropped)?;
                return Err(error);
            }
        };
        let written = {
            let mut stream = match self.stream.lock() {
                Ok(stream) => stream,
                Err(_poisoned) => {
                    count_drop(&self.dropped)?;
                    return Err(io::Error::other("the durable trace mutex is poisoned"));
                }
            };
            stream.as_mut().map_or_else(
                || Err(io::Error::other("trace sink is closed")),
                |file| file.write_all(&line).and_then(|()| file.sync_data()),
            )
        };
        if let Err(error) = written {
            count_drop(&self.dropped)?;
            return Err(error);
        }
        Ok(())
    }

    /// How many events could not be written.
    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }

    /// Flushes and closes the stream. Everything written afterwards fails, which is the reachable form of "the disk is gone".
    ///
    /// # Errors
    /// The failure to flush.
    pub fn close(&self) -> io::Result<()> {
        let file = self
            .stream
            .lock()
            .map_err(|_poisoned| io::Error::other("the durable trace mutex is poisoned"))?
            .take();
        if let Some(file) = file {
            file.sync_all()?;
        }
        let dropped = self.dropped();
        if dropped == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "the durable trace lost {dropped} event(s)"
            )))
        }
    }
}

fn count_drop(counter: &AtomicU64) -> io::Result<()> {
    counter
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |held| {
            held.checked_add(1)
        })
        .map(|_previous| ())
        .map_err(|_maximum| io::Error::other("the trace drop counter overflowed"))
}
