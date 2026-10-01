// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An explicitly injected supervision clock whose events belong to one child.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The clock a supervisor reads, carried through cancellation children.
#[derive(Debug, Clone)]
pub struct Clock {
    events: Option<PathBuf>,
}

impl Clock {
    /// The operating system's monotonic clock.
    #[must_use]
    pub const fn wall() -> Self {
        Self { events: None }
    }

    /// Virtual elapsed milliseconds published by each child under its PID, acknowledged by supervision.
    #[must_use]
    #[cfg(any(test, feature = "testkit"))]
    pub const fn events(directory: PathBuf) -> Self {
        Self {
            events: Some(directory),
        }
    }

    pub(super) fn now(&self, started: Instant, pid: u32) -> Instant {
        self.read(started, pid).0
    }

    pub(super) fn read(&self, started: Instant, pid: u32) -> (Instant, Option<Vec<u8>>) {
        let wall = Instant::now();
        let Some(directory) = &self.events else {
            return (wall, None);
        };
        let path = directory.join(pid.to_string());
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            return (wall, None);
        };
        if !metadata.is_file() || metadata.len() > 32 {
            return (wall, None);
        }
        let Ok(bytes) = std::fs::read(path) else {
            return (wall, None);
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return (wall, None);
        };
        let Ok(millis) = text.parse::<u64>() else {
            return (wall, None);
        };
        match started.checked_add(Duration::from_millis(millis)) {
            Some(advanced) => (advanced.max(wall), Some(bytes)),
            None => (wall, None),
        }
    }

    pub(super) fn acknowledged(&self, pid: u32, value: Option<&[u8]>) -> std::io::Result<()> {
        if let (Some(directory), Some(value)) = (&self.events, value) {
            std::fs::write(directory.join(format!("{pid}.ack")), value)?;
        }
        Ok(())
    }

    pub(super) fn finished(&self, pid: u32) {
        if let Some(directory) = &self.events {
            for name in [pid.to_string(), format!("{pid}.ack")] {
                match std::fs::remove_file(directory.join(name)) {
                    Ok(()) => {}
                    Err(_cleanup_after_completion_is_best_effort) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, Instant};
    use std::time::Duration;

    #[test]
    fn a_child_event_advances_the_bound_without_waiting_on_the_wall_clock() {
        let directory = tempfile::tempdir().expect("clock events");
        let clock = Clock::events(directory.path().to_path_buf());
        let started = Instant::now();
        std::fs::write(directory.path().join("7"), "60000").expect("an elapsed minute");
        assert!(
            clock.now(started, 7) >= started + Duration::from_secs(60),
            "a declared elapsed minute must advance supervision immediately"
        );
        assert!(
            clock.now(started, 8) < started + Duration::from_secs(60),
            "another child's event changes no clock of this child"
        );
    }
}
