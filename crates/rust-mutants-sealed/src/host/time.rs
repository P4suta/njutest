// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guest's clocks: fixed origins moved only by fuel spent and by waits, and `poll_oneoff`, which waits by moving them at once.

use crate::abi::{
    CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME, EVENT_SIZE,
    EVENTTYPE_CLOCK, EVENTTYPE_FD_READ, EVENTTYPE_FD_WRITE, Errno, SUBCLOCKFLAGS_ABSTIME,
    SUBSCRIPTION_SIZE,
};
use crate::imports::WasiFunction;
use crate::transcript::RefusalReason;

use super::fs::Readiness;
use super::memory::GuestMemory;
use super::{Failure, Host, HostStop, Params, RESOLUTION};

/// `clock_res_get`: the one resolution every clock has.
pub(crate) fn resolution(memory: &mut GuestMemory<'_>, clock: u32, at: u32) -> Result<(), Failure> {
    match clock {
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_PROCESS_CPUTIME | CLOCK_THREAD_CPUTIME => {
            Ok(memory.write_u64(at, RESOLUTION)?)
        }
        _ => Err(Errno::Inval.into()),
    }
}

/// One event `poll_oneoff` answers with.
#[derive(Debug, Clone, Copy)]
struct Event {
    /// The subscription's own tag for it.
    userdata: u64,
    /// The error the event carries.
    errno: Errno,
    /// Which kind of subscription it answers.
    kind: u8,
    /// How many bytes a descriptor has ready.
    bytes: u64,
    /// The descriptor's event flags.
    flags: u16,
}

impl Event {
    /// A clock's deadline, passed.
    const fn clock(userdata: u64, errno: Errno) -> Self {
        Self {
            userdata,
            errno,
            kind: EVENTTYPE_CLOCK,
            bytes: 0,
            flags: 0,
        }
    }
}

/// One clock subscription waited on: its tag, and how long until its deadline, or nothing for one the clock never reaches.
#[derive(Debug, Clone, Copy)]
struct Waiting {
    /// The subscription's own tag for it.
    userdata: u64,
    /// How long until its deadline, where the clock ever reaches it.
    left: Option<u64>,
}

/// One clock subscription as the guest wrote it.
#[derive(Debug, Clone, Copy)]
struct ClockSubscription {
    /// The subscription's own tag for it.
    userdata: u64,
    /// Which clock.
    clock: u32,
    /// The deadline, relative to now or absolute as the flags say.
    timeout: u64,
    /// The subscription's clock flags.
    flags: u16,
}

/// What a poll has read of its subscriptions so far.
#[derive(Debug)]
struct Subscribed {
    /// The events that are ready now.
    ready: Vec<Event>,
    /// The clocks waited on.
    waiting: Vec<Waiting>,
}

impl Host {
    /// What `clock` reads now: its origin, moved by the fuel spent and by every wait.
    pub(crate) fn now(&self, clock: u32) -> Result<u64, Failure> {
        let ran = self
            .spent
            .checked_mul(self.clock.nanos_per_fuel.get())
            .ok_or(Errno::Overflow)?;
        let origin = match clock {
            CLOCK_REALTIME => self.clock.realtime_origin,
            CLOCK_MONOTONIC => self.clock.monotonic_origin,
            CLOCK_PROCESS_CPUTIME | CLOCK_THREAD_CPUTIME => return Ok(ran),
            _ => return Err(Errno::Inval.into()),
        };
        origin
            .checked_add(ran)
            .and_then(|now| now.checked_add(self.waited))
            .ok_or(Failure::Errno(Errno::Overflow))
    }

    /// `poll_oneoff`: every descriptor is ready at once, and where none is asked about the clocks move straight to the earliest deadline.
    pub(crate) fn poll(
        &mut self,
        memory: &mut GuestMemory<'_>,
        params: &Params<'_>,
    ) -> Result<(), Failure> {
        let count = params.w(2)?;
        if count == 0 {
            return Err(Errno::Inval.into());
        }
        let mut subscribed = Subscribed {
            ready: Vec::new(),
            waiting: Vec::new(),
        };
        for index in 0..count {
            let index = usize::try_from(index).map_err(|_wide| Errno::Fault)?;
            let at = GuestMemory::offset(
                params.w(0)?,
                index.checked_mul(SUBSCRIPTION_SIZE).ok_or(Errno::Fault)?,
            )?;
            self.subscribe(memory, at, &mut subscribed)?;
        }
        let Subscribed { mut ready, waiting } = subscribed;
        let waited = if ready.is_empty() {
            let Some(wait) = waiting.iter().filter_map(|clock| clock.left).min() else {
                return Err(Failure::Stop(HostStop::FuelExhausted));
            };
            self.waited = self
                .waited
                .checked_add(wait)
                .ok_or(Failure::Stop(HostStop::FuelExhausted))?;
            wait
        } else {
            0
        };
        ready.extend(
            waiting
                .iter()
                .filter(|clock| clock.left.is_some_and(|left| left <= waited))
                .map(|clock| Event::clock(clock.userdata, Errno::Success)),
        );
        for (index, event) in ready.iter().enumerate() {
            let at = GuestMemory::offset(
                params.w(1)?,
                index.checked_mul(EVENT_SIZE).ok_or(Errno::Fault)?,
            )?;
            write_event(memory, at, event)?;
        }
        let answered = u32::try_from(ready.len()).map_err(|_wide| Errno::Overflow)?;
        Ok(memory.write_u32(params.w(3)?, answered)?)
    }

    /// Reads the subscription at `at` into the events ready now or the clocks waited on.
    fn subscribe(
        &mut self,
        memory: &mut GuestMemory<'_>,
        at: u32,
        subscribed: &mut Subscribed,
    ) -> Result<(), Failure> {
        let userdata = memory.read_u64(at)?;
        let kind = memory.read_u8(GuestMemory::offset(at, 8)?)?;
        let body = GuestMemory::offset(at, 16)?;
        match kind {
            EVENTTYPE_CLOCK => {
                let subscription = ClockSubscription {
                    userdata,
                    clock: memory.read_u32(body)?,
                    timeout: memory.read_u64(GuestMemory::offset(body, 8)?)?,
                    flags: memory.read_u16(GuestMemory::offset(body, 24)?)?,
                };
                self.wait_on(subscription, subscribed)
            }
            EVENTTYPE_FD_READ | EVENTTYPE_FD_WRITE => {
                let fd = memory.read_u32(body)?;
                let readiness = if kind == EVENTTYPE_FD_READ {
                    Readiness::Read
                } else {
                    Readiness::Write
                };
                let (errno, bytes, flags) = match self.files.ready(fd, readiness) {
                    Ok((bytes, flags)) => (Errno::Success, bytes, flags),
                    Err(errno) => (errno, 0, 0),
                };
                subscribed.ready.push(Event {
                    userdata,
                    errno,
                    kind,
                    bytes,
                    flags,
                });
                Ok(())
            }
            _ => Err(Errno::Inval.into()),
        }
    }

    /// Adds a clock subscription to what is waited on, refusing one on a clock of the time run.
    fn wait_on(
        &mut self,
        subscription: ClockSubscription,
        subscribed: &mut Subscribed,
    ) -> Result<(), Failure> {
        let userdata = subscription.userdata;
        match subscription.clock {
            CLOCK_REALTIME | CLOCK_MONOTONIC => {
                let now = self.now(subscription.clock)?;
                let deadline = if u32::from(subscription.flags) & SUBCLOCKFLAGS_ABSTIME == 0 {
                    now.checked_add(subscription.timeout)
                } else {
                    Some(subscription.timeout)
                };
                subscribed.waiting.push(Waiting {
                    userdata,
                    left: deadline.map(|deadline| deadline.saturating_sub(now)),
                });
            }
            CLOCK_PROCESS_CPUTIME | CLOCK_THREAD_CPUTIME => {
                self.refuse(WasiFunction::PollOneoff, RefusalReason::CpuClockWait)?;
                subscribed
                    .ready
                    .push(Event::clock(userdata, RefusalReason::CpuClockWait.errno()));
            }
            _ => subscribed.ready.push(Event::clock(userdata, Errno::Inval)),
        }
        Ok(())
    }
}

/// Writes `event` at `at`.
fn write_event(memory: &mut GuestMemory<'_>, at: u32, event: &Event) -> Result<(), Failure> {
    let mut record = [0_u8; EVENT_SIZE];
    let fields: [(usize, &[u8]); 5] = [
        (0, &event.userdata.to_le_bytes()),
        (8, &event.errno.number().to_le_bytes()),
        (10, &[event.kind]),
        (16, &event.bytes.to_le_bytes()),
        (24, &event.flags.to_le_bytes()),
    ];
    for (offset, field) in fields {
        let end = offset.checked_add(field.len()).ok_or(Errno::Fault)?;
        record
            .get_mut(offset..end)
            .ok_or(Errno::Fault)?
            .copy_from_slice(field);
    }
    Ok(memory.write(at, &record)?)
}
