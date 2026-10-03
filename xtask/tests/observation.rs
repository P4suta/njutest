// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A producer's backlog failure stays observable through its terminal publication.

#![expect(
    clippy::panic_in_result_fn,
    reason = "test assertions report a mismatched actual observation by panicking"
)]

use xtask::observation::{Event, Observation};

#[test]
fn a_stalled_observation_reader_retains_the_bounded_overflow_refusal() -> std::io::Result<()> {
    let observation = Observation::subscribe();
    for _publication in 0..4096 {
        observation.signal().publish(Event::Changed);
    }
    let waited = observation.wait("task-producer", "output", None)?;
    let refusal = waited
        .event
        .expect_err("an unlimited backlog cannot prove complete observation");
    assert!(refusal.to_string().contains("full"), "{refusal}");
    assert!(waited.note.machine.cpus > 0);
    Ok(())
}

#[test]
fn terminal_completion_cannot_erase_a_prior_observation_overflow() -> std::io::Result<()> {
    let observation = Observation::subscribe();
    let producer = observation.signal();
    for _publication in 0..4096 {
        producer.publish(Event::Changed);
    }
    producer.publish(Event::Completed);
    for _read in 0..2 {
        let waited = observation.wait("task-producer", "completion", None)?;
        assert!(
            waited.event.is_err(),
            "terminal completion erased lost evidence"
        );
    }
    Ok(())
}

#[derive(Debug)]
struct SemanticClock(std::cell::Cell<std::time::Instant>);

impl xtask::observation::Clock for SemanticClock {
    fn now(&self) -> std::time::Instant {
        self.0.get()
    }

    fn park(&self, remaining: Option<std::time::Duration>) -> std::io::Result<()> {
        let remaining = remaining.ok_or_else(|| std::io::Error::other("no injected deadline"))?;
        let next = self
            .0
            .get()
            .checked_add(remaining)
            .ok_or_else(|| std::io::Error::other("the injected deadline overflowed"))?;
        self.0.set(next);
        Ok(())
    }
}

#[test]
fn the_injected_deadline_expires_at_equality_without_sampling_the_host() -> std::io::Result<()> {
    use xtask::observation::{Clock as _, Waiting};
    let clock = SemanticClock(std::cell::Cell::new(std::time::Instant::now()));
    let deadline = clock
        .now()
        .checked_add(std::time::Duration::from_nanos(10))
        .ok_or_else(|| std::io::Error::other("the semantic deadline does not fit"))?;
    let observation = Observation::subscribe();
    let waited = observation.wait_with(
        Waiting {
            owner: "semantic-window",
            cause: "deadline",
            deadline: Some(deadline),
        },
        &clock,
    )?;
    assert_eq!(waited.event?, Event::Deadline);
    assert_eq!(clock.now(), deadline);
    assert!(waited.note.machine.cpus > 0);
    Ok(())
}

#[test]
fn an_actual_publication_precedes_the_injected_deadline() -> std::io::Result<()> {
    use xtask::observation::{Clock as _, Waiting};
    let clock = SemanticClock(std::cell::Cell::new(std::time::Instant::now()));
    let observation = Observation::subscribe();
    observation.signal().publish(Event::Completed);
    let deadline = clock.now();
    let waited = observation.wait_with(
        Waiting {
            owner: "actual-producer",
            cause: "completion",
            deadline: Some(deadline),
        },
        &clock,
    )?;
    assert_eq!(waited.event?, Event::Completed);
    assert_eq!(clock.now(), deadline);
    Ok(())
}

#[derive(Debug)]
struct ConcurrentFailureClock(xtask::observation::Signal);

impl xtask::observation::Clock for ConcurrentFailureClock {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    fn park(&self, _remaining: Option<std::time::Duration>) -> std::io::Result<()> {
        std::thread::scope(|scope| {
            let publisher = njutest_devkit::thread::ScopedThread::launch(scope, || {
                self.0.publish(Event::Completed);
                self.0.failed(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "the actual producer lost its completion evidence",
                ));
            });
            publisher
                .join()
                .map_err(|_panic| std::io::Error::other("the actual publication panicked"))
        })
    }
}

#[test]
fn concurrent_completion_cannot_erase_a_retained_producer_failure() -> std::io::Result<()> {
    let observation = Observation::subscribe();
    let clock = ConcurrentFailureClock(observation.signal());
    for _decision in 0..2 {
        let waited = observation.wait_with(
            xtask::observation::Waiting {
                owner: "actual-concurrent-task-producer",
                cause: "complete evidence or producer refusal",
                deadline: None,
            },
            &clock,
        )?;
        let refusal = waited
            .event
            .expect_err("queued completion erased a concurrent producer failure");
        assert_eq!(refusal.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            refusal.to_string().contains("lost its completion evidence"),
            "{refusal}"
        );
    }
    Ok(())
}

#[test]
fn an_open_log_publishes_each_write_before_its_producer_closes_it() -> std::io::Result<()> {
    use std::io::{Seek as _, Write as _};
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("waiting.log");
    let mut writer = std::fs::File::create(&path)?;
    let observed = Observation::filesystem(directory.path(), true)?;
    std::thread::scope(|scope| {
        const PENDING_LOG_WRITES: usize = 1;
        let (send, received) = std::sync::mpsc::sync_channel::<String>(PENDING_LOG_WRITES);
        let producer = njutest_devkit::thread::ScopedThread::launch(scope, move || {
            for text in received {
                writer.set_len(0)?;
                writer.rewind()?;
                writer.write_all(text.as_bytes())?;
                writer.flush()?;
            }
            Ok::<(), std::io::Error>(())
        });
        let result = (|| {
            for text in [
                "waiting for the first lane\n",
                "waiting for the next lane\n",
            ] {
                send.send(text.to_owned()).map_err(std::io::Error::other)?;
                let deadline = Instant::now()
                    .checked_add(Duration::from_secs(2))
                    .ok_or_else(|| std::io::Error::other("the native log deadline overflowed"))?;
                loop {
                    let waited = observed.wait(
                        "open-lane-progress-file",
                        "actual producer write before descriptor closure",
                        Some(deadline),
                    )?;
                    eprintln!(
                        "{}",
                        serde_json::to_string(&waited.note).map_err(std::io::Error::other)?
                    );
                    match waited.event? {
                        Event::Changed => {}
                        event @ (Event::Completed | Event::Cancelled | Event::Deadline) => {
                            return Err(std::io::Error::other(format!(
                                "the open producer log holds {:?}, but the native reader received {event:?} instead of a write wake",
                                std::fs::read_to_string(&path)?
                            )));
                        }
                    }
                    if std::fs::read_to_string(&path)? == text {
                        break;
                    }
                }
            }
            Ok::<(), std::io::Error>(())
        })();
        drop(send);
        producer.join().map_err(std::io::Error::other)??;
        result
    })
}

#[test]
fn a_new_external_log_publishes_writes_while_its_producer_keeps_it_open() -> std::io::Result<()> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir()?;
    let turns = directory.path().join("turns");
    std::fs::create_dir_all(&turns)?;
    let observed = Observation::filesystem(directory.path(), true)?;
    let path = turns.join("waiting.log");
    let mut command = Command::new(njutest_devkit::paths::posix_sh());
    command
        .args([
            "-c",
            "while IFS= read -r text; do printf '%s\\n' \"$text\" >&2; done",
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::from(std::fs::File::create(&path)?));
    let mut producer = njutest_devkit::process::SupervisedChild::launch(&mut command)
        .map_err(std::io::Error::other)?;
    let mut input = producer
        .take_stdin()
        .ok_or_else(|| std::io::Error::other("the actual producer has no control descriptor"))?;
    let result = (|| {
        for text in ["first lane", "next lane", "final lane"] {
            writeln!(input, "{text}")?;
            input.flush()?;
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(2))
                .ok_or_else(|| std::io::Error::other("the native log deadline overflowed"))?;
            loop {
                let waited = observed.wait(
                    "external-open-lane-progress-file",
                    "actual producer write before descriptor closure",
                    Some(deadline),
                )?;
                eprintln!(
                    "{}",
                    serde_json::to_string(&waited.note).map_err(std::io::Error::other)?
                );
                match waited.event? {
                    Event::Changed => {}
                    event @ (Event::Completed | Event::Cancelled | Event::Deadline) => {
                        return Err(std::io::Error::other(format!(
                            "the external open producer log holds {:?}, but the native reader received {event:?} instead of a write wake",
                            std::fs::read_to_string(&path)?
                        )));
                    }
                }
                if std::fs::read_to_string(&path)?.ends_with(&format!("{text}\n")) {
                    break;
                }
            }
        }
        Ok::<(), std::io::Error>(())
    })();
    drop(input);
    let ended = producer.wait().map_err(std::io::Error::other)?;
    assert!(
        ended.success(),
        "the released actual producer failed: {ended}"
    );
    result
}
