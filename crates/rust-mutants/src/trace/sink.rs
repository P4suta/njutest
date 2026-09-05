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

/// Keeps the events of a recording.
///
/// A sink that cannot keep an event answers with an error and, if it counts,
/// counts the loss; it never fails the run. Every method takes `&self`
/// because the recorder is shared between threads.
pub trait Sink: Send + Sync {
    /// Keeps one event.
    ///
    /// # Errors
    ///
    /// The reason the event was not kept. The recorder counts it and moves on.
    fn emit(&self, event: &Event) -> io::Result<()>;

    /// How many events this sink lost, when it is the authority on that:
    /// a full ring absorbs a loss without an error, and only the sink knows.
    /// `None` leaves the count to the recorder's observed failures.
    fn dropped(&self) -> Option<u64> {
        None
    }

    /// Releases what the sink holds. Called once by the recorder at the end
    /// of the recording; closing twice is not an error.
    ///
    /// # Errors
    ///
    /// The failure to close, which the recorder drops.
    fn close(&self) -> io::Result<()> {
        Ok(())
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

impl Sink for MemorySink {
    fn emit(&self, event: &Event) -> io::Result<()> {
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
        Ok(())
    }

    fn dropped(&self) -> Option<u64> {
        Some(self.dropped.load(Ordering::SeqCst))
    }

    fn close(&self) -> io::Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Writes JSON Lines to any writer, one flush per event, so a run that hangs
/// or is killed still leaves everything it recorded readable.
#[derive(Debug)]
pub struct WriterSink<W: Write + Send> {
    writer: Mutex<W>,
    dropped: AtomicU64,
}

impl<W: Write + Send> WriterSink<W> {
    /// Wraps a writer.
    pub const fn new(writer: W) -> Self {
        Self {
            writer: Mutex::new(writer),
            dropped: AtomicU64::new(0),
        }
    }
}

impl<W: Write + Send> Sink for WriterSink<W> {
    fn emit(&self, event: &Event) -> io::Result<()> {
        let line = encode(event).inspect_err(|_| {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        })?;
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        writer
            .write_all(&line)
            .and_then(|()| writer.flush())
            .inspect_err(|_| {
                self.dropped.fetch_add(1, Ordering::SeqCst);
            })
    }

    fn dropped(&self) -> Option<u64> {
        Some(self.dropped.load(Ordering::SeqCst))
    }

    fn close(&self) -> io::Result<()> {
        self.writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flush()
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
        #[allow(
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

impl Sink for DirSink {
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

    fn dropped(&self) -> Option<u64> {
        Some(self.dropped.load(Ordering::SeqCst))
    }

    fn close(&self) -> io::Result<()> {
        let file = self
            .stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        file.map_or(Ok(()), |file| file.sync_all())
    }
}
