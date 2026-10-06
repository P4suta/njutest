// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned non-reaping process completion events.

use std::io;
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug)]
struct Completion {
    answer: Option<Result<(), Arc<io::Error>>>,
    subscribers: Vec<Weak<Wake>>,
}

#[derive(Debug)]
struct State {
    completion: Mutex<Completion>,
    changed: Condvar,
}

impl State {
    const fn pending() -> Self {
        Self {
            completion: Mutex::new(Completion {
                answer: None,
                subscribers: Vec::new(),
            }),
            changed: Condvar::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ExitNotice {
    Completed,
    ObservationStopped,
}

#[derive(Debug)]
struct Wake(mpsc::SyncSender<ExitNotice>);

impl Wake {
    fn publish(&self) -> bool {
        match self.0.try_send(ExitNotice::Completed) {
            Ok(()) | Err(mpsc::TrySendError::Full(ExitNotice::Completed)) => true,
            Err(
                mpsc::TrySendError::Full(ExitNotice::ObservationStopped)
                | mpsc::TrySendError::Disconnected(
                    ExitNotice::Completed | ExitNotice::ObservationStopped,
                ),
            ) => false,
        }
    }
}

/// An owned, finite subscription to a leader's one terminal observation.
#[derive(Debug)]
pub struct ExitSubscription {
    received: mpsc::Receiver<ExitNotice>,
    wake: Arc<Wake>,
    state: Arc<State>,
}

impl ExitSubscription {
    /// Reads the retained terminal result after a publication, including an already published refusal.
    ///
    /// # Errors
    /// The producer disconnected before publishing its terminal observation.
    pub fn wait(&self) -> io::Result<bool> {
        match self.received.recv().map_err(io::Error::other)? {
            ExitNotice::Completed => {
                let completion = self.state.completion.lock().map_err(poisoned)?;
                match &completion.answer {
                    Some(Ok(())) => Ok(true),
                    Some(Err(source)) => Err(io::Error::new(source.kind(), Arc::clone(source))),
                    None => Err(io::Error::other(
                        "the exit publication preceded its retained terminal result",
                    )),
                }
            }
            ExitNotice::ObservationStopped => Ok(false),
        }
    }

    /// The endpoint that ends only this observation bridge, without claiming process completion.
    #[must_use]
    pub fn stopper(&self) -> ExitStop {
        ExitStop {
            wake: Arc::clone(&self.wake),
        }
    }
}

/// An explicit stop endpoint for an owned completion observation bridge.
#[derive(Debug)]
pub struct ExitStop {
    wake: Arc<Wake>,
}

impl ExitStop {
    /// Wakes the bridge so its owner can join it without cancelling the observed process.
    pub fn stop(&self) {
        match self.wake.0.try_send(ExitNotice::ObservationStopped) {
            Ok(())
            | Err(
                mpsc::TrySendError::Full(ExitNotice::Completed | ExitNotice::ObservationStopped)
                | mpsc::TrySendError::Disconnected(
                    ExitNotice::Completed | ExitNotice::ObservationStopped,
                ),
            ) => {}
        }
    }
}

/// An exit event that leaves the leader waitable until its process set settles.
#[derive(Debug)]
pub struct ChildEvent {
    state: Arc<State>,
    worker: Mutex<EventThread>,
}

impl ChildEvent {
    pub(crate) fn of(child: &Child) -> io::Result<Self> {
        let process = super::sys::ExitHandle::of(child)?;
        let state = Arc::new(State::pending());
        let worker = EventThread::launch(process, Arc::clone(&state))?;
        Ok(Self {
            state,
            worker: Mutex::new(worker),
        })
    }

    /// Registers a finite publication endpoint before inspecting the terminal state.
    ///
    /// # Errors
    /// The retained terminal state could not be locked.
    pub fn subscribe(&self) -> io::Result<ExitSubscription> {
        let (sent, received) = mpsc::sync_channel(1);
        let wake = Arc::new(Wake(sent));
        let mut completion = self.state.completion.lock().map_err(poisoned)?;
        completion
            .subscribers
            .retain(|subscriber| subscriber.strong_count() != 0);
        completion.subscribers.push(Arc::downgrade(&wake));
        if completion.answer.is_some() && !wake.publish() {
            return Err(io::Error::other(
                "the newly owned exit subscription disconnected",
            ));
        }
        drop(completion);
        Ok(ExitSubscription {
            received,
            wake,
            state: Arc::clone(&self.state),
        })
    }

    /// Waits for the owned exit event or its semantic completion backstop.
    ///
    /// # Errors
    /// The operating system, observer or owned completion state failed.
    pub fn wait(&self, bound: Option<Duration>) -> io::Result<bool> {
        let completion = self.state.completion.lock().map_err(poisoned)?;
        let completion = match bound {
            Some(bound) => {
                self.state
                    .changed
                    .wait_timeout_while(completion, bound, |completion| completion.answer.is_none())
                    .map_err(poisoned)?
                    .0
            }
            None => self
                .state
                .changed
                .wait_while(completion, |completion| completion.answer.is_none())
                .map_err(poisoned)?,
        };
        match &completion.answer {
            Some(Ok(())) => Ok(true),
            Some(Err(source)) => Err(io::Error::new(source.kind(), Arc::clone(source))),
            None => Ok(false),
        }
    }

    pub(crate) fn finish(&self) -> io::Result<()> {
        self.worker.lock().map_err(poisoned)?.join()
    }
}

impl Drop for ChildEvent {
    fn drop(&mut self) {
        if let Err(source) = self.finish() {
            super::group::terminal(&format!("the exit event owner could not join: {source}"));
        }
    }
}

#[derive(Debug)]
struct EventThread {
    handle: Option<JoinHandle<()>>,
}

impl EventThread {
    fn launch(process: super::sys::ExitHandle, state: Arc<State>) -> io::Result<Self> {
        let handle = std::thread::Builder::new()
            .name("njutest-process-exit".to_owned())
            .spawn(move || {
                let published = Publishing(state);
                published.answer(process.wait().map_err(Arc::new));
            })?;
        Ok(Self {
            handle: Some(handle),
        })
    }

    fn join(&mut self) -> io::Result<()> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle
            .join()
            .map_err(|_panic| io::Error::other("the owned process observer panicked"))
    }
}

impl Drop for EventThread {
    fn drop(&mut self) {
        if let Err(source) = self.join() {
            super::group::terminal(&format!("the process observer could not join: {source}"));
        }
    }
}

struct Publishing(Arc<State>);

impl Publishing {
    fn answer(&self, answer: Result<(), Arc<io::Error>>) {
        let mut completion = match self.0.completion.lock() {
            Ok(completion) => completion,
            Err(source) => super::group::terminal(&format!(
                "process completion publication was poisoned: {source}"
            )),
        };
        completion.answer = Some(answer);
        self.0.changed.notify_all();
        completion
            .subscribers
            .retain(|subscriber| match subscriber.upgrade() {
                Some(subscriber) => subscriber.publish(),
                None => false,
            });
    }
}

impl Drop for Publishing {
    fn drop(&mut self) {
        let missing = match self.0.completion.lock() {
            Ok(completion) => completion.answer.is_none(),
            Err(source) => super::group::terminal(&format!(
                "process completion disposal was poisoned: {source}"
            )),
        };
        if missing {
            self.answer(Err(Arc::new(io::Error::other(
                "the process observer ended before publishing",
            ))));
        }
    }
}

fn poisoned<T>(_source: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("the owned process completion event was poisoned")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    #[test]
    fn an_observer_lost_before_publication_cannot_be_a_successful_exit_wake() {
        let child = super::ChildEvent {
            state: Arc::new(super::State::pending()),
            worker: Mutex::new(super::EventThread { handle: None }),
        };
        let subscribed = child
            .subscribe()
            .expect("the actual publication subscriber");
        let lost = super::Publishing(Arc::clone(&child.state));
        drop(lost);
        let error = subscribed
            .wait()
            .expect_err("the lost observer publication must retain its terminal refusal");
        assert!(
            error.to_string().contains("ended before publishing"),
            "{error}"
        );
        let late = child.subscribe().expect("the late terminal subscriber");
        assert_eq!(
            late.wait()
                .expect_err("the retained refusal is sticky")
                .to_string(),
            error.to_string()
        );
    }
}
