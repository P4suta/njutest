// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A ready-path invalidation subscription retained before its owned child starts.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, Thread};
use std::time::Instant;

use notify::Watcher as _;

/// The observed marker or non-reaping process completion that released a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    /// The subscribed path is a regular file.
    Ready,
    /// The retained child exit event completed before its marker appeared.
    ChildExited,
}

/// A native path subscription whose wake tokens coalesce invalidation while refusals remain sticky.
pub struct ReadyPath {
    path: PathBuf,
    events: Arc<Events>,
    _watcher: notify::RecommendedWatcher,
}

impl std::fmt::Debug for ReadyPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadyPath")
            .field("path", &self.path)
            .field("reader", &self.events.reader)
            .finish_non_exhaustive()
    }
}

impl ReadyPath {
    /// Subscribes to the marker's parent before the caller starts its producer.
    ///
    /// # Errors
    /// The parent or its native subscription could not be established.
    pub fn subscribe(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("the ready path has no subscribable parent"))?;
        let events = Arc::new(Events {
            reader: thread::current(),
            failure: Mutex::new(None),
        });
        let published = Arc::clone(&events);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if event.kind.is_access() => {}
                Ok(_changed) => published.reader.unpark(),
                Err(source) => published.failed(io::Error::other(source)),
            })
            .map_err(io::Error::other)?;
        watcher
            .watch(parent, notify::RecursiveMode::NonRecursive)
            .map_err(io::Error::other)?;
        Ok(Self {
            path: path.to_path_buf(),
            events,
            _watcher: watcher,
        })
    }

    /// Waits on actual path or retained child events without consuming the child's output owner.
    ///
    /// # Errors
    /// The marker, child event, observer or unchanged semantic deadline refused the wait.
    pub fn wait(
        &self,
        child: &super::SupervisedChild,
        deadline: Instant,
    ) -> io::Result<ReadyState> {
        if self.events.reader.id() != thread::current().id() {
            return Err(io::Error::other(
                "a ready path must be awaited on the thread that subscribed",
            ));
        }
        let completion = child.completion().map_err(io::Error::other)?;
        let bridge = ExitBridge::launch(&completion, Arc::clone(&self.events))?;
        let outcome = loop {
            self.events.inspect()?;
            match std::fs::symlink_metadata(&self.path) {
                Ok(metadata) if metadata.is_file() => break Ok(ReadyState::Ready),
                Ok(_other) => {
                    break Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the ready marker is not a regular file",
                    ));
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => break Err(source),
            }
            if completion.wait(Some(std::time::Duration::ZERO))? {
                break Ok(ReadyState::ChildExited);
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the owned child reached neither its ready marker nor completion before the deadline",
                ));
            };
            let began = Instant::now();
            thread::park_timeout(left);
            self.record(began)?;
        };
        bridge.finish()?;
        outcome
    }

    fn record(&self, began: Instant) -> io::Result<()> {
        let elapsed_ns = u64::try_from(began.elapsed().as_nanos()).map_err(io::Error::other)?;
        let note = serde_json::json!({
            "kind": "host-wait",
            "payload": {
                "owner": format!("ready-path:{}", self.path.display()),
                "cause": "filesystem-or-owned-child-exit-or-semantic-deadline",
                "elapsed_ns": elapsed_ns,
                "machine": {
                    "os": std::env::consts::OS,
                    "cpus": thread::available_parallelism()?.get()
                }
            }
        });
        eprintln!(
            "{}",
            serde_json::to_string(&note).map_err(io::Error::other)?
        );
        Ok(())
    }
}

struct Events {
    reader: Thread,
    failure: Mutex<Option<Arc<io::Error>>>,
}

impl Events {
    fn failed(&self, source: io::Error) {
        match self.failure.lock() {
            Ok(mut failure) => {
                if failure.is_none() {
                    *failure = Some(Arc::new(source));
                }
            }
            Err(source) => {
                eprintln!("ready-path failure publication was poisoned: {source}");
                std::process::abort();
            }
        }
        self.reader.unpark();
    }

    fn inspect(&self) -> io::Result<()> {
        let failure = self
            .failure
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        match failure.as_ref() {
            Some(source) => Err(io::Error::new(source.kind(), Arc::clone(source))),
            None => Ok(()),
        }
    }
}

struct ExitBridge {
    thread: Option<crate::thread::JoinedThread<()>>,
    stop: njutest_process::ExitStop,
}

impl ExitBridge {
    fn launch(child: &njutest_process::ChildEvent, events: Arc<Events>) -> io::Result<Self> {
        let subscribed = child.subscribe()?;
        let stop = subscribed.stopper();
        let thread = crate::thread::JoinedThread::launch(move || match subscribed.wait() {
            Ok(true) => events.reader.unpark(),
            Ok(false) => {}
            Err(source) => events.failed(source),
        });
        Ok(Self {
            thread: Some(thread),
            stop,
        })
    }

    fn finish(mut self) -> io::Result<()> {
        self.join()
    }

    fn join(&mut self) -> io::Result<()> {
        self.stop.stop();
        match self.thread.take() {
            Some(thread) => thread.join().map_err(io::Error::other),
            None => Ok(()),
        }
    }
}

impl Drop for ExitBridge {
    fn drop(&mut self) {
        if let Err(source) = self.join() {
            eprintln!("the owned ready-path exit bridge could not join: {source}");
            std::process::abort();
        }
    }
}
