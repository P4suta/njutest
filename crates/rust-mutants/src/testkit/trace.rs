// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Recording a run the same way twice, so a golden can freeze what it says.

#![expect(
    clippy::panic,
    reason = "the frozen origin is a constant of this file: a moment it cannot parse is a \
              broken testkit, not a run-time condition a caller could handle"
)]

use jiff::Timestamp;

use crate::trace::{Clock, Event, MemorySink, Recorder, Sink};

/// The moment a frozen recording starts at: a round number far from any real one, so a golden that leaks a wall clock is obvious.
pub const ORIGIN: i64 = 1_800_000_000;

/// A clock that advances one second per reading from [`ORIGIN`], so a recording is the same bytes every time it is made.
///
/// # Panics
/// Never: [`ORIGIN`] is a moment.
#[must_use]
pub fn stepping_clock() -> Clock {
    let origin = Timestamp::from_second(ORIGIN)
        .unwrap_or_else(|error| panic!("the frozen origin is a moment: {error}"));
    Clock::stepping(origin, std::time::Duration::from_secs(1))
}

/// A recorder over a memory sink on the stepping clock, which is what a test reads back with [`Recorder::events`].
#[must_use]
pub fn memory_recorder() -> Recorder {
    Recorder::new(Sink::Memory(MemorySink::unbounded()), stepping_clock())
}

/// The type name of every event, in order: what a test asserts when it is about which decisions were recorded rather than about what each one said.
#[must_use]
pub fn type_names(events: &[Event]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| event.payload.type_name())
        .collect()
}
