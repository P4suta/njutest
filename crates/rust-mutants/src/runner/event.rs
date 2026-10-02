// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Retained native exit and resource subscriptions for one execution.

use std::io;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crate::observation::{Event, Observation, Signal};
pub(super) use njutest_process::ChildEvent;
use notify::Watcher as _;

/// A bridge whose own stop and join do not release the group's kernel exit owner.
#[derive(Debug)]
pub(super) struct ProcessWake {
    _bridge: ExitBridge,
}

#[derive(Debug)]
struct ExitBridge {
    handle: Option<JoinHandle<()>>,
    stop: njutest_process::ExitStop,
}

impl ProcessWake {
    pub(super) fn launch(child: &ChildEvent, signal: Signal) -> io::Result<Self> {
        ExitBridge::launch(child.subscribe()?, signal).map(|bridge| Self { _bridge: bridge })
    }
}

impl ExitBridge {
    fn launch(subscribed: njutest_process::ExitSubscription, signal: Signal) -> io::Result<Self> {
        let stop = subscribed.stopper();
        let handle = std::thread::Builder::new()
            .name("native-exit-publication".to_owned())
            .spawn(move || match subscribed.wait() {
                Ok(true) => signal.publish(Event::Completed),
                Ok(false) => {}
                Err(source) => signal.failed(source),
            })?;
        Ok(Self {
            handle: Some(handle),
            stop,
        })
    }
    fn finish(&mut self) {
        self.stop.stop();
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            eprintln!("the native exit publication bridge panicked before its owned join");
            std::process::abort();
        }
    }
}

impl Drop for ExitBridge {
    fn drop(&mut self) {
        self.finish();
    }
}

pub(super) struct Resources {
    _watcher: Option<notify::RecommendedWatcher>,
    _cancel: crate::observation::Cancellation,
}

impl Resources {
    pub(super) fn subscribe(
        observation: &Observation,
        stops: &super::Stops<'_>,
    ) -> io::Result<Self> {
        let mut roots = std::collections::BTreeSet::new();
        if let Some(path) = stops.monitor {
            add_parent(&mut roots, path)?;
        }
        if let Some(progress) = stops.progress {
            for path in progress.signals() {
                add_parent(&mut roots, path)?;
            }
        }
        if let Some(directory) = stops.cancel.clock.directory() {
            roots.insert(directory.to_path_buf());
        }
        let watcher = if roots.is_empty() {
            None
        } else {
            let signal = observation.signal();
            let mut watcher = notify::recommended_watcher(
                move |event: notify::Result<notify::Event>| match event {
                    Ok(event) if event.kind.is_access() => {}
                    Ok(_changed) => signal.publish(Event::Changed),
                    Err(source) => signal.failed(io::Error::other(source)),
                },
            )
            .map_err(io::Error::other)?;
            for root in roots {
                watcher
                    .watch(&root, notify::RecursiveMode::NonRecursive)
                    .map_err(io::Error::other)?;
            }
            Some(watcher)
        };
        let cancel = observation.cancellation(stops.cancel)?;
        Ok(Self {
            _watcher: watcher,
            _cancel: cancel,
        })
    }
}

fn add_parent(roots: &mut std::collections::BTreeSet<PathBuf>, path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("the monitored resource has no subscribable parent"))?;
    roots.insert(parent.to_path_buf());
    Ok(())
}
