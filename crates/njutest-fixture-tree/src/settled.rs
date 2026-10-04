// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A file's stamp stands for its content only once its newest time is older than the clock's tick at the moment the stamp was taken.

use std::time::{Duration, SystemTime};

/// How much later than one write a filesystem may still date another the same: FAT's two seconds, and a second for the coarse clock a write is dated by to lag the one [`Taken::take`] reads.
pub const GRANULARITY: Duration = Duration::from_secs(3);

/// The leave to compare two stamps, which only [`Taken`] gives, so no stamp is compared without asking whether it has settled.
#[derive(Debug, Clone, Copy)]
pub struct Comparing(());

/// What a file's metadata says about the generation of its content.
pub trait Stamp {
    /// The newest time this stamp carries, or `None` where it carries no change time, the one time no writer can set back.
    fn newest(&self) -> Option<SystemTime>;

    /// Whether every field of the two stamps agrees, which only [`Taken`] can ask.
    fn same(&self, other: &Self, by: Comparing) -> bool;
}

/// A stamp and the moment the clock read just before it was taken.
#[derive(Debug, Clone)]
pub struct Taken<S> {
    stamp: S,
    moment: SystemTime,
}

impl<S: Stamp> Taken<S> {
    /// Reads the clock and then the stamp `read` takes, so every write the stamp misses happens after the moment.
    ///
    /// # Errors
    ///
    /// Whatever `read` fails with.
    pub fn take<E>(read: impl FnOnce() -> Result<S, E>) -> Result<Self, E> {
        let moment = SystemTime::now();
        let stamp = read()?;
        Ok(Self { stamp, moment })
    }

    /// The stamp a record says was taken at `moment`.
    #[must_use]
    pub const fn recorded(stamp: S, moment: SystemTime) -> Self {
        Self { stamp, moment }
    }

    /// The stamp, for its fields.
    #[must_use]
    pub const fn stamp(&self) -> &S {
        &self.stamp
    }

    /// The moment the clock read just before the stamp was taken.
    #[must_use]
    pub const fn moment(&self) -> SystemTime {
        self.moment
    }

    /// The stamp and its moment, for a record to hold.
    #[must_use]
    pub fn into_parts(self) -> (S, SystemTime) {
        (self.stamp, self.moment)
    }

    /// Whether the stamp's newest time was older than its moment by more than [`GRANULARITY`], so that every later write is dated newer.
    #[must_use]
    pub fn settled(&self) -> bool {
        match self
            .stamp
            .newest()
            .and_then(|newest| newest.checked_add(GRANULARITY))
        {
            Some(ripe) => ripe < self.moment,
            None => false,
        }
    }

    /// Whether a file stamped `current` still holds what it held when this stamp was taken, which only a settled stamp can say.
    #[must_use]
    pub fn holds(&self, current: &S) -> bool {
        self.settled() && self.stamp.same(current, Comparing(()))
    }

    /// Whether `current` differs from this stamp, which proves the file changed, where agreeing proves nothing until the stamp has settled.
    #[must_use]
    pub fn changed(&self, current: &S) -> bool {
        !self.stamp.same(current, Comparing(()))
    }
}

/// The time `seconds` and `nanoseconds` after the Unix epoch, as Unix spells a change time.
#[must_use]
pub fn since_unix_epoch(seconds: i64, nanoseconds: i64) -> Option<SystemTime> {
    let nanoseconds = match u64::try_from(nanoseconds) {
        Ok(nanoseconds) => Duration::from_nanos(nanoseconds),
        Err(_negative) => return None,
    };
    let whole = Duration::from_secs(seconds.unsigned_abs());
    let second = if seconds < 0 {
        SystemTime::UNIX_EPOCH.checked_sub(whole)
    } else {
        SystemTime::UNIX_EPOCH.checked_add(whole)
    }?;
    second.checked_add(nanoseconds)
}

/// The time `ticks` hundreds of nanoseconds after 1601 began, as Windows spells a change time, or `None` for the zero a filesystem without one reports.
#[must_use]
pub fn since_windows_epoch(ticks: i64) -> Option<SystemTime> {
    const PER_SECOND: u64 = 10_000_000;
    const BEFORE_UNIX: Duration = Duration::from_hours(3_234_576);
    let ticks = match u64::try_from(ticks) {
        Ok(ticks) if ticks > 0 => ticks,
        Ok(_) | Err(_) => return None,
    };
    let since = Duration::from_secs(ticks.checked_div(PER_SECOND)?).checked_add(
        Duration::from_nanos(ticks.checked_rem(PER_SECOND)?.checked_mul(100)?),
    )?;
    SystemTime::UNIX_EPOCH
        .checked_sub(BEFORE_UNIX)?
        .checked_add(since)
}

#[cfg(test)]
mod tests {
    use super::{Comparing, GRANULARITY, Stamp, Taken, since_unix_epoch, since_windows_epoch};
    use std::time::{Duration, SystemTime};

    #[derive(Debug, Clone, Copy)]
    struct Written {
        newest: Option<SystemTime>,
        bytes: u8,
    }

    impl Stamp for Written {
        fn newest(&self) -> Option<SystemTime> {
            self.newest
        }

        fn same(&self, other: &Self, _: Comparing) -> bool {
            self.newest == other.newest && self.bytes == other.bytes
        }
    }

    fn written(at: SystemTime, bytes: u8) -> Written {
        Written {
            newest: Some(at),
            bytes,
        }
    }

    fn later(at: SystemTime, by: Duration) -> SystemTime {
        at.checked_add(by).expect("a representable later time")
    }

    fn earlier(at: SystemTime, by: Duration) -> SystemTime {
        at.checked_sub(by).expect("a representable earlier time")
    }

    fn moment() -> SystemTime {
        later(SystemTime::UNIX_EPOCH, Duration::from_hours(500_000))
    }

    #[test]
    fn a_stamp_settles_only_once_its_newest_time_is_older_than_its_moment_by_the_granularity() {
        let at = moment();
        let stamp = written(at, 1);
        for (taken, settled) in [
            (at, false),
            (later(at, Duration::from_millis(1)), false),
            (later(at, GRANULARITY), false),
            (later(later(at, GRANULARITY), Duration::from_nanos(1)), true),
            (later(at, Duration::from_hours(1)), true),
            (earlier(at, Duration::from_hours(1)), false),
        ] {
            assert_eq!(
                Taken::recorded(stamp, taken).settled(),
                settled,
                "taken {:?} after its newest time",
                taken.duration_since(at)
            );
        }
        let timeless = Written {
            newest: None,
            bytes: 1,
        };
        assert!(
            !Taken::recorded(timeless, later(at, Duration::from_hours(1))).settled(),
            "a stamp without a change time never settles"
        );
    }

    #[test]
    fn only_a_settled_stamp_holds_and_any_difference_is_a_change() {
        let at = moment();
        let racy = Taken::recorded(written(at, 1), at);
        let settled = Taken::recorded(written(at, 1), later(at, Duration::from_mins(1)));
        assert!(
            !racy.holds(&written(at, 1)),
            "an unsettled stamp holds nothing"
        );
        assert!(settled.holds(&written(at, 1)));
        assert!(!settled.holds(&written(at, 2)));
        for taken in [&racy, &settled] {
            assert!(!taken.changed(&written(at, 1)));
            assert!(taken.changed(&written(at, 2)));
            assert!(taken.changed(&written(later(at, Duration::from_nanos(1)), 1)));
        }
    }

    #[test]
    fn the_clock_is_read_before_the_stamp_and_a_failed_read_is_passed_on() {
        let mut read = None;
        let taken = Taken::take(|| {
            let now = SystemTime::now();
            read = Some(now);
            Ok::<_, std::io::Error>(written(now, 1))
        })
        .expect("a stamp read");
        let read = read.expect("the stamp was read");
        assert!(taken.moment() <= read);
        assert!(
            !taken.settled(),
            "a stamp dated when it was taken has not settled"
        );
        let refused = Taken::<Written>::take(|| Err(std::io::Error::other("unreadable")))
            .expect_err("the read's failure");
        assert_eq!(refused.to_string(), "unreadable");
    }

    #[test]
    fn unix_and_windows_change_times_are_read_from_their_own_epochs() {
        let epoch = SystemTime::UNIX_EPOCH;
        assert_eq!(since_unix_epoch(0, 0), Some(epoch));
        assert_eq!(
            since_unix_epoch(1, 5),
            Some(later(epoch, Duration::new(1, 5)))
        );
        assert_eq!(
            since_unix_epoch(-1, 5),
            Some(later(
                earlier(epoch, Duration::from_secs(1)),
                Duration::from_nanos(5)
            ))
        );
        assert_eq!(since_unix_epoch(1, -1), None);
        assert_eq!(since_windows_epoch(116_444_736_000_000_000), Some(epoch));
        assert_eq!(
            since_windows_epoch(116_444_736_010_000_001),
            Some(later(epoch, Duration::new(1, 100)))
        );
        assert_eq!(since_windows_epoch(0), None);
        assert_eq!(since_windows_epoch(-1), None);
    }
}
