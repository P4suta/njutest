// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a recording goes: memory, a writer, or a directory.

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

/// Caps one preserved output file. A capture larger than the limit is cut to
/// it; the event still digests the whole capture.
pub const OUTPUT_FILE_LIMIT: usize = 1 << 20;

/// Ends a preserved output file that did not fit.
pub const TRUNCATION_MARKER: &str = "...";

/// Where a recording goes.
///
/// A closed set, so this is an enum rather than a trait object: the sinks a
/// run can have are the sinks this engine ships, dispatch is a match the
/// compiler checks, and a tee holds its sinks directly rather than a vector
/// of allocations behind vtables.
///
/// A sink that cannot keep an event answers with an error and, where it
/// knows, counts the loss; it never fails the run.
#[derive(Debug)]
#[non_exhaustive]
pub enum Sink {
    /// The most recent events in memory: the sink of a test and of an
    /// in-process reader.
    Memory(MemorySink),
    /// A directory of JSON Lines, with the commands' output beside it.
    Dir(DirSink),
    /// Several at once, in order.
    ///
    /// One sink failing costs that sink the event and not the others.
    Tee(Vec<Self>),
}

impl Sink {
    /// Keeps one event.
    ///
    /// # Errors
    ///
    /// The reason the event was not kept. The recorder counts it and moves
    /// on.
    pub fn emit(&self, event: &Event) -> io::Result<()> {
        match self {
            Self::Memory(sink) => {
                sink.emit(event);
                Ok(())
            }
            Self::Dir(sink) => sink.emit(event),
            Self::Tee(sinks) => {
                // Every sink is offered the event, whatever the ones before
                // it did: a full disk must not cost the ring the last thing
                // the run recorded.
                let mut kept = false;
                for sink in sinks {
                    kept |= sink.emit(event).is_ok();
                }
                if kept {
                    Ok(())
                } else {
                    Err(io::Error::other("no trace sink kept the event"))
                }
            }
        }
    }

    /// How many events this sink lost, when it is the authority on that.
    #[must_use]
    pub fn dropped(&self) -> Option<u64> {
        match self {
            Self::Memory(sink) => Some(sink.dropped()),
            Self::Dir(sink) => Some(sink.dropped()),
            Self::Tee(sinks) => sinks.iter().filter_map(Self::dropped).min(),
        }
    }

    /// Every event a memory sink of this recording kept, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        match self {
            Self::Memory(sink) => sink.events(),
            Self::Dir(_) => Vec::new(),
            Self::Tee(sinks) => sinks
                .iter()
                .map(Self::events)
                .find(|events| !events.is_empty())
                .unwrap_or_default(),
        }
    }

    /// Whether every part of this sink has been released.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Memory(sink) => sink.is_closed(),
            Self::Dir(sink) => sink.is_closed(),
            Self::Tee(sinks) => sinks.iter().all(Self::is_closed),
        }
    }

    /// Releases what the sink holds.
    ///
    /// # Errors
    ///
    /// The failure to close, which the recorder drops.
    pub fn close(&self) -> io::Result<()> {
        match self {
            Self::Memory(sink) => {
                sink.close();
                Ok(())
            }
            Self::Dir(sink) => sink.close(),
            Self::Tee(sinks) => sinks.iter().try_for_each(Self::close),
        }
    }
}

/// Keeps the most recent events in memory: the sink of a test and of an
/// in-process reader. A full ring drops its oldest event and counts it.
#[derive(Debug)]
pub struct MemorySink {
    capacity: Option<usize>,
    events: Mutex<VecDeque<Event>>,
    dropped: AtomicU64,
    closed: AtomicBool,
}

impl MemorySink {
    /// Keeps everything.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self::with_capacity(None)
    }

    /// Keeps the newest `capacity` events.
    #[must_use]
    pub const fn bounded(capacity: usize) -> Self {
        Self::with_capacity(Some(capacity))
    }

    const fn with_capacity(capacity: Option<usize>) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::new()),
            dropped: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// The kept events, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Whether [`Sink::close`] was called.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl MemorySink {
    /// Keeps one event, dropping the oldest when the ring is full. Memory
    /// does not fail: what a full ring loses it counts.
    fn emit(&self, event: &Event) {
        let evicted = {
            let mut events = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut evicted: u64 = 0;
            match self.capacity {
                Some(0) => evicted = 1,
                Some(capacity) => {
                    while events.len() >= capacity {
                        events.pop_front();
                        evicted = evicted.saturating_add(1);
                    }
                    events.push_back(event.clone());
                }
                None => events.push_back(event.clone()),
            }
            evicted
        };
        self.dropped.fetch_add(evicted, Ordering::SeqCst);
    }

    /// How many events the ring evicted.
    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
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

/// Writes a recording to a directory: the stream in [`FILE_NAME`] and the
/// output of the commands that produced any under [`OUTPUT_DIRECTORY_NAME`].
///
/// The directory belongs to one recording and is created exclusively, so a
/// name another recording owns is refused rather than joined: everything in a
/// recording is numbered from the first event, and a second run sharing a
/// directory would append to the first run's stream and write its output over
/// the files the first run's events digested.
#[derive(Debug)]
pub struct DirSink {
    directory: PathBuf,
    stream: Mutex<Option<File>>,
    output_exists: AtomicBool,
    dropped: AtomicU64,
}

impl DirSink {
    /// Creates `directory` and opens its stream.
    ///
    /// # Errors
    ///
    /// The failure to create the directory (including `AlreadyExists`) or to
    /// open the stream. A caller that cannot trace runs untraced.
    pub fn create(directory: &Path) -> io::Result<Self> {
        #[expect(
            clippy::create_dir,
            reason = "exclusive creation is the point: a directory another recording owns is refused"
        )]
        fs::create_dir(directory)?;
        let stream = File::options()
            .create(true)
            .append(true)
            .open(directory.join(FILE_NAME))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            stream: Mutex::new(Some(stream)),
            output_exists: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
        })
    }

    /// The directory of this recording.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Whether the stream has been released.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    }

    /// Writes the captured output of an exec event beside the stream and
    /// returns the event pointing at the file. Best effort: an output that
    /// cannot be written costs its path, not the event. The event is copied
    /// so a sink sharing it with others never leaks this sink's paths.
    fn preserve_output(&self, event: &Event) -> Option<Event> {
        let Payload::Exec { exec } = &event.payload else {
            return None;
        };
        if exec.output.is_empty() {
            return None;
        }
        let (data, truncated) = limit_output(&exec.output);
        let name = format!("{}.txt", event.seq);
        self.write_output(&name, &data).ok()?;
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
            fs::create_dir_all(&directory)?;
            self.output_exists.store(true, Ordering::SeqCst);
        }
        fs::write(directory.join(name), data)
    }
}

/// Caps a capture at [`OUTPUT_FILE_LIMIT`], marking what it cut.
fn limit_output(output: &[u8]) -> (Vec<u8>, bool) {
    match output.get(..OUTPUT_FILE_LIMIT) {
        Some(head) if output.len() > OUTPUT_FILE_LIMIT => {
            let mut limited =
                Vec::with_capacity(OUTPUT_FILE_LIMIT.saturating_add(TRUNCATION_MARKER.len()));
            limited.extend_from_slice(head);
            limited.extend_from_slice(TRUNCATION_MARKER.as_bytes());
            (limited, true)
        }
        _ => (output.to_vec(), false),
    }
}

impl DirSink {
    /// Writes one event, preserving the output of a command that produced
    /// any.
    fn emit(&self, event: &Event) -> io::Result<()> {
        let preserved = self.preserve_output(event);
        let line = encode(preserved.as_ref().unwrap_or(event)).inspect_err(|_| {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        })?;
        let written = {
            let mut stream = self
                .stream
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // The line is in the file's hands and readable; whether it
            // reached the disk is not something a diagnostic stream fails
            // over.
            stream.as_mut().map_or_else(
                || Err(io::Error::other("trace sink is closed")),
                |file| file.write_all(&line).map(|()| drop(file.sync_data())),
            )
        };
        if written.is_err() {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
        written
    }

    /// How many events could not be written.
    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }

    /// Flushes and closes the stream. Everything written afterwards fails,
    /// which is the reachable form of "the disk is gone".
    ///
    /// # Errors
    ///
    /// The failure to flush.
    pub fn close(&self) -> io::Result<()> {
        let file = self
            .stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        file.map_or(Ok(()), |file| file.sync_all())
    }
}
