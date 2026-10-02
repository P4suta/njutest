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
