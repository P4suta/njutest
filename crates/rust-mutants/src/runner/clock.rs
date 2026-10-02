// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An explicitly injected supervision clock whose events belong to one child.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The clock a supervisor reads, carried through cancellation children.
#[derive(Debug, Clone)]
pub struct Clock {
    source: ClockSource,
}

/// The two clock domains, whose observations never fall back into one another.
#[derive(Debug, Clone)]
enum ClockSource {
    Wall,
    Events(PathBuf),
}

impl Clock {
    /// The operating system's monotonic clock.
    #[must_use]
    pub const fn wall() -> Self {
        Self {
            source: ClockSource::Wall,
        }
    }

    /// Virtual elapsed milliseconds published by each child under its PID, acknowledged by supervision.
    #[must_use]
    #[cfg(any(test, feature = "testkit"))]
    pub const fn events(directory: PathBuf) -> Self {
        Self {
            source: ClockSource::Events(directory),
        }
    }

    pub(super) fn directory(&self) -> Option<&std::path::Path> {
        match &self.source {
            ClockSource::Wall => None,
            ClockSource::Events(directory) => Some(directory),
        }
    }

    pub(super) fn host_deadline(&self, left: Option<Duration>) -> io::Result<Option<Instant>> {
        match &self.source {
            ClockSource::Wall => left
                .map(|left| {
                    Instant::now().checked_add(left).ok_or_else(|| {
                        io::Error::other("the native observation deadline exceeds the clock")
                    })
                })
                .transpose(),
            ClockSource::Events(_directory) => Ok(None),
        }
    }

    pub(super) fn now(&self, started: Instant, pid: u32) -> io::Result<Instant> {
        self.read(started, pid).map(|(now, _event)| now)
    }

    pub(super) fn read(
        &self,
        started: Instant,
        pid: u32,
    ) -> io::Result<(Instant, Option<Vec<u8>>)> {
        match &self.source {
            ClockSource::Wall => Ok((Instant::now(), None)),
            ClockSource::Events(directory) => Self::logical(directory, started, pid),
        }
    }

    /// Reads one bounded, no-follow logical publication or reports its exact refusal.
    fn logical(
        directory: &std::path::Path,
        started: Instant,
        pid: u32,
    ) -> io::Result<(Instant, Option<Vec<u8>>)> {
        let bytes = match super::read_side_channel(&directory.join(pid.to_string())) {
            Ok(bytes) => bytes,
            Err(missing) if missing.kind() == io::ErrorKind::NotFound => {
                return Ok((started, None));
            }
            Err(source) => return Err(source),
        };
        if bytes.len() > 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid logical clock event",
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
        let millis = text.parse::<u64>().map_err(io::Error::other)?;
        let advanced = started
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "logical clock event exceeds Instant",
                )
            })?;
        Ok((advanced, Some(bytes)))
    }

    pub(super) fn acknowledged(&self, pid: u32, value: Option<&[u8]>) -> io::Result<()> {
        if let (ClockSource::Events(directory), Some(value)) = (&self.source, value) {
            let pending = directory.join(format!("{pid}.ack.next"));
            std::fs::write(&pending, value)?;
            std::fs::rename(pending, directory.join(format!("{pid}.ack")))?;
        }
        Ok(())
    }

    pub(super) fn finished(&self, pid: u32) -> io::Result<()> {
        let mut failures = Vec::new();
        if let ClockSource::Events(directory) = &self.source {
            for name in [
                pid.to_string(),
                format!("{pid}.ack"),
                format!("{pid}.ack.next"),
            ] {
                let path = directory.join(name);
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(missing) if missing.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => failures.push(format!("{}: {source}", path.display())),
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(failures.join("; ")))
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
            clock.now(started, 7).expect("logical clock") >= started + Duration::from_secs(60),
            "a declared elapsed minute must advance supervision immediately"
        );
        assert!(
            clock.now(started, 8).expect("logical clock") < started + Duration::from_secs(60),
            "another child's event changes no clock of this child"
        );
    }

    #[test]
    fn an_injected_clock_without_an_event_does_not_advance_with_host_time() {
        let directory = tempfile::tempdir().expect("clock events");
        let clock = Clock::events(directory.path().to_path_buf());
        let started = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("the host clock holds an earlier origin");
        assert_eq!(clock.now(started, 7).expect("logical clock"), started);
        std::fs::write(directory.path().join("7"), "1").expect("one logical millisecond");
        assert_eq!(
            clock.now(started, 7).expect("logical clock"),
            started + Duration::from_millis(1)
        );
    }

    #[test]
    fn a_refused_clock_event_is_not_replaced_with_host_time() {
        let directory = tempfile::tempdir().expect("clock events");
        let clock = Clock::events(directory.path().to_path_buf());
        let started = Instant::now();
        for bytes in [b"invalid".as_slice(), b"", b"\xff"] {
            std::fs::write(directory.path().join("7"), bytes).expect("an actual malformed event");
            clock
                .read(started, 7)
                .expect_err("the actual refused event cannot become host time");
        }
        std::fs::remove_file(directory.path().join("7")).expect("the malformed event is removed");
        std::fs::create_dir_all(directory.path().join("7")).expect("an actual non-file event");
        clock
            .read(started, 7)
            .expect_err("the actual refused event cannot become host time");
    }
}
