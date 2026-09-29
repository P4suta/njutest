// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! When a watched process counts as stalled, and how often one that is moving is told to say so, so that one that moves at least once a quiet window never is.

use core::time::Duration;

/// How many beats a process is told to fit into one quiet window.
pub const BEATS_PER_WINDOW: u32 = 4;

/// The shortest interval a process is told to beat at.
pub const SHORTEST_BEAT: Duration = Duration::from_millis(1);

/// How long a process spending a reservation may go before it says it is moving: a quarter of the window, and never less than [`SHORTEST_BEAT`].
#[must_use]
pub fn beat_every(quiet: Duration) -> Duration {
    let share = match quiet.checked_div(BEATS_PER_WINDOW) {
        Some(share) => share,
        None => quiet,
    };
    share.max(SHORTEST_BEAT)
}

/// When a watched process last moved, measured from when the watch began, and how long it may stay still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stillness {
    moved: Duration,
    quiet: Duration,
}

impl Stillness {
    /// A watch that counts the moment it began as the process's last movement.
    #[must_use]
    pub const fn new(quiet: Duration) -> Self {
        Self {
            moved: Duration::ZERO,
            quiet,
        }
    }

    /// The watch after a look at `now` that saw a signal change, or saw none.
    #[must_use]
    pub const fn looked(self, now: Duration, changed: bool) -> Self {
        if changed {
            Self { moved: now, ..self }
        } else {
            self
        }
    }

    /// The moment the process counts as stalled unless it moves first, where that moment can be said.
    #[must_use]
    pub const fn stalls_at(self) -> Option<Duration> {
        self.moved.checked_add(self.quiet)
    }

    /// Whether the process has been still for the whole window at `now`, which a stall has to be before anything else is asked.
    #[must_use]
    pub fn still_for_the_window(self, now: Duration) -> bool {
        now.saturating_sub(self.moved) >= self.quiet
    }
}

#[cfg(test)]
mod tests;
