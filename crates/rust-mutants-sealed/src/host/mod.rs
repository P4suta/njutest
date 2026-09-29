// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The WASI preview1 host: the state one invocation runs against, and the one exhaustive match every import is carried out through.

mod fs;
mod io;
mod limiter;
mod memory;
mod overlay;
mod time;

use std::collections::BTreeMap;
use std::time::Instant;

use wasmtime::{Caller, Val};

use crate::abi::{CLOCK_REALTIME, Errno, RIGHTS_FD_WRITE};
use crate::error::Invariant;
use crate::imports::WasiFunction;
use crate::interrupt::Interrupt;
use crate::invocation::{ClockPolicy, Invocation};
use crate::random::RandomStream;
use crate::transcript::{Captured, Denials, OverlayEntry, Refusal, RefusalReason};

use self::fs::{Fault, Filesystem, Stream, TimesRequest};
use self::memory::{Access, GuestMemory};

pub(crate) use self::limiter::{Limiter, TABLE_ELEMENTS};

/// The fuel every host call costs before it does anything.
pub(crate) const CALL_FUEL: u64 = 64;

/// The fuel every byte of guest memory a host call reads or writes costs.
pub(crate) const BYTE_FUEL: u64 = 1;

/// The time every file and directory of a snapshot carries until the guest sets another.
pub(crate) const FILE_TIME: u64 = 0;

/// The resolution every clock reports, in nanoseconds.
pub(crate) const RESOLUTION: u64 = 1;

/// Why the host stopped the guest, which the runner reads before anything wasmtime says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    variant_size_differences,
    reason = "an exit code is four bytes and an invariant one; boxing the code to even out the ratio would make every stop an allocation"
)]
pub(crate) enum HostStop {
    /// The guest called `proc_exit`.
    Exited {
        /// The code it exited with.
        code: u32,
    },
    /// The guest's fuel ran out in a host call, or it waited for a time the clock never reaches.
    FuelExhausted,
    /// The wall-clock watchdog expired.
    WatchdogExpired,
    /// Whoever ran the guest stopped.
    Interrupted,
    /// The host broke an invariant of its own.
    Broken(Invariant),
}

/// How a host call failed, before the failure becomes an error number or a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// An ordinary error number.
    Errno(Errno),
    /// A refusal the transcript records.
    Refused(RefusalReason),
    /// A stop of the guest.
    Stop(HostStop),
}

impl From<Errno> for Failure {
    fn from(errno: Errno) -> Self {
        Self::Errno(errno)
    }
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Errno(errno) => Self::Errno(errno),
            Fault::Refused(reason) => Self::Refused(reason),
        }
    }
}

impl From<Invariant> for Failure {
    fn from(invariant: Invariant) -> Self {
        Self::Stop(HostStop::Broken(invariant))
    }
}

impl From<Access> for Failure {
    fn from(access: Access) -> Self {
        match access {
            Access::Fault => Self::Errno(Errno::Fault),
            Access::Unpaid => Self::Stop(HostStop::FuelExhausted),
        }
    }
}

/// Everything the host holds for one invocation.
#[derive(Debug)]
pub(crate) struct Host {
    /// The arguments, each a C string.
    arguments: Vec<Vec<u8>>,
    /// The environment, each variable a `NAME=value` C string.
    environment: Vec<Vec<u8>>,
    /// How the clocks read.
    clock: ClockPolicy,
    /// The fuel the invocation began with.
    budget: u64,
    /// The fuel spent before the host call being carried out, which is what the clocks read by.
    spent: u64,
    /// The guest's random bytes.
    random: RandomStream,
    /// The filesystem and the descriptors.
    files: Filesystem,
    /// Standard output.
    stdout: Captured,
    /// Standard error.
    stderr: Captured,
    /// The caps of standard output and standard error.
    caps: (u64, u64),
    /// Every refusal, by function and reason.
    refusals: BTreeMap<(WasiFunction, RefusalReason), u64>,
    /// How far waits have moved the clocks.
    waited: u64,
    /// The ceilings on memory and tables.
    pub(crate) limiter: Limiter,
    /// The guest's exported memory, once there is an instance.
    pub(crate) memory: Option<wasmtime::Memory>,
    /// Why the host stopped the guest, once it has.
    pub(crate) stop: Option<HostStop>,
    /// When the watchdog expires, where it does.
    deadline: Option<Instant>,
    /// What stops the guest when whoever runs it stops.
    interrupt: Interrupt,
}

/// What the host hands the runner once the guest has stopped.
#[derive(Debug)]
pub(crate) struct Ended {
    /// Standard output.
    pub(crate) stdout: Captured,
    /// Standard error.
    pub(crate) stderr: Captured,
    /// Every refusal, by function and reason.
    pub(crate) refusals: Vec<Refusal>,
    /// What the ceilings refused.
    pub(crate) denials: Denials,
    /// The largest the linear memory grew.
    pub(crate) peak_memory: u64,
    /// How far waits moved the clocks.
    pub(crate) waited: u64,
    /// Every path whose final state differs from its snapshot.
    pub(crate) overlay: Vec<OverlayEntry>,
}

impl Host {
    /// The host for `invocation`, the watchdog expiring at `deadline` where it does, and every call refused once `interrupt` is raised.
    pub(crate) fn new(
        invocation: &Invocation,
        (deadline, interrupt): (Option<Instant>, Interrupt),
    ) -> Result<Self, Invariant> {
        let arguments = invocation
            .arguments
            .as_slice()
            .iter()
            .map(|argument| c_string(argument.as_bytes()))
            .collect();
        let environment = invocation
            .environment
            .variables()
            .map(|(name, value)| c_string(format!("{name}={value}").as_bytes()))
            .collect();
        let files = Filesystem::new(&invocation.preopens, invocation.limits.overlay)
            .map_err(|_numbering| Invariant::Width)?;
        Ok(Self {
            arguments,
            environment,
            clock: invocation.clock,
            budget: invocation.fuel,
            spent: 0,
            random: RandomStream::new(invocation.seed),
            files,
            stdout: Captured::new(),
            stderr: Captured::new(),
            caps: (invocation.limits.stdout, invocation.limits.stderr),
            refusals: BTreeMap::new(),
            waited: 0,
            limiter: Limiter::new(invocation.limits.memory),
            memory: None,
            stop: None,
            deadline,
            interrupt,
        })
    }

    /// Counts one refusal of `function` for `reason`.
    fn refuse(&mut self, function: WasiFunction, reason: RefusalReason) -> Result<(), Invariant> {
        let count = self.refusals.entry((function, reason)).or_insert(0);
        *count = count.checked_add(1).ok_or(Invariant::Width)?;
        Ok(())
    }

    /// What the host hands the runner once the guest has stopped.
    pub(crate) fn end(self) -> Result<Ended, Invariant> {
        let overlay = self
            .files
            .overlay()
            .map_err(|_unreadable| Invariant::Width)?;
        Ok(Ended {
            stdout: self.stdout,
            stderr: self.stderr,
            refusals: self
                .refusals
                .into_iter()
                .map(|((function, reason), count)| Refusal {
                    function,
                    reason,
                    count,
                })
                .collect(),
            denials: self.limiter.denials,
            peak_memory: self.limiter.peak,
            waited: self.waited,
            overlay,
        })
    }

    /// Carries out `function` with `params` against `memory`.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per WASI function: the table is the function, and splitting it would split the one match that keeps it total"
    )]
    fn perform(
        &mut self,
        function: WasiFunction,
        params: &Params<'_>,
        memory: &mut GuestMemory<'_>,
    ) -> Result<(), Failure> {
        match function {
            WasiFunction::ArgsGet => {
                io::strings_get(memory, &self.arguments, params.w(0)?, params.w(1)?)
            }
            WasiFunction::ArgsSizesGet => {
                io::strings_sizes(memory, &self.arguments, params.w(0)?, params.w(1)?)
            }
            WasiFunction::EnvironGet => {
                io::strings_get(memory, &self.environment, params.w(0)?, params.w(1)?)
            }
            WasiFunction::EnvironSizesGet => {
                io::strings_sizes(memory, &self.environment, params.w(0)?, params.w(1)?)
            }
            WasiFunction::ClockResGet => time::resolution(memory, params.w(0)?, params.w(1)?),
            WasiFunction::ClockTimeGet => {
                let now = self.now(params.w(0)?)?;
                Ok(memory.write_u64(params.w(2)?, now)?)
            }
            WasiFunction::FdAdvise => Ok(self.files.advise(params.w(0)?, params.w(3)?)?),
            WasiFunction::FdAllocate => {
                Ok(self
                    .files
                    .allocate(params.w(0)?, params.l(1)?, params.l(2)?)?)
            }
            WasiFunction::FdClose => Ok(self.files.close(params.w(0)?)?),
            WasiFunction::FdDatasync | WasiFunction::FdSync => Ok(self.files.sync(params.w(0)?)?),
            WasiFunction::FdFdstatGet => {
                io::fdstat(&self.files, memory, params.w(0)?, params.w(1)?)
            }
            WasiFunction::FdFdstatSetFlags => {
                Ok(self.files.set_flags(params.w(0)?, params.w(1)?)?)
            }
            WasiFunction::FdFdstatSetRights => {
                Ok(self
                    .files
                    .set_rights(params.w(0)?, params.l(1)?, params.l(2)?)?)
            }
            WasiFunction::FdFilestatGet => {
                let stat = self.files.filestat(params.w(0)?)?;
                io::write_filestat(memory, params.w(1)?, &stat)
            }
            WasiFunction::FdFilestatSetSize => {
                Ok(self.files.set_size(params.w(0)?, params.l(1)?)?)
            }
            WasiFunction::FdFilestatSetTimes => {
                let request = self.times(params.l(1)?, params.l(2)?, params.w(3)?)?;
                Ok(self.files.set_times(params.w(0)?, request)?)
            }
            WasiFunction::FdPread => io::pread(&self.files, memory, params),
            WasiFunction::FdPrestatGet => {
                io::prestat(&self.files, memory, params.w(0)?, params.w(1)?)
            }
            WasiFunction::FdPrestatDirName => io::prestat_name(&self.files, memory, params),
            WasiFunction::FdPwrite => io::pwrite(&mut self.files, memory, params),
            WasiFunction::FdRead => io::read(&mut self.files, memory, params),
            WasiFunction::FdReaddir => io::readdir(&self.files, memory, params),
            WasiFunction::FdRenumber => Ok(self.files.renumber(params.w(0)?, params.w(1)?)?),
            WasiFunction::FdSeek => {
                let delta = params.l(1)?.cast_signed();
                let to = self.files.seek(params.w(0)?, delta, params.w(2)?)?;
                Ok(memory.write_u64(params.w(3)?, to)?)
            }
            WasiFunction::FdTell => {
                let at = self.files.tell(params.w(0)?)?;
                Ok(memory.write_u64(params.w(1)?, at)?)
            }
            WasiFunction::FdWrite => self.write(memory, params),
            WasiFunction::PathCreateDirectory => {
                let path = io::path(memory, params.w(1)?, params.w(2)?)?;
                Ok(self.files.create_directory(params.w(0)?, &path)?)
            }
            WasiFunction::PathFilestatGet => {
                let path = io::path(memory, params.w(2)?, params.w(3)?)?;
                let stat = self.files.path_filestat(params.w(0)?, &path)?;
                io::write_filestat(memory, params.w(4)?, &stat)
            }
            WasiFunction::PathFilestatSetTimes => {
                let path = io::path(memory, params.w(2)?, params.w(3)?)?;
                let request = self.times(params.l(4)?, params.l(5)?, params.w(6)?)?;
                Ok(self.files.path_set_times(params.w(0)?, &path, request)?)
            }
            WasiFunction::PathLink | WasiFunction::PathSymlink => {
                Err(Failure::Refused(RefusalReason::Link))
            }
            WasiFunction::PathOpen => io::open(&mut self.files, memory, params),
            WasiFunction::PathReadlink => {
                let path = io::path(memory, params.w(1)?, params.w(2)?)?;
                Err(self.files.readlink(params.w(0)?, &path).into())
            }
            WasiFunction::PathRemoveDirectory => {
                let path = io::path(memory, params.w(1)?, params.w(2)?)?;
                Ok(self.files.remove_directory(params.w(0)?, &path)?)
            }
            WasiFunction::PathRename => {
                let from = io::path(memory, params.w(1)?, params.w(2)?)?;
                let to = io::path(memory, params.w(4)?, params.w(5)?)?;
                Ok(self
                    .files
                    .rename((params.w(0)?, &from), (params.w(3)?, &to))?)
            }
            WasiFunction::PathUnlinkFile => {
                let path = io::path(memory, params.w(1)?, params.w(2)?)?;
                Ok(self.files.unlink_file(params.w(0)?, &path)?)
            }
            WasiFunction::PollOneoff => self.poll(memory, params),
            WasiFunction::ProcExit => Err(Failure::Stop(HostStop::Exited { code: params.w(0)? })),
            WasiFunction::ProcRaise => Err(Failure::Refused(RefusalReason::Signal)),
            WasiFunction::SchedYield => Ok(()),
            WasiFunction::RandomGet => {
                let len = usize::try_from(params.w(1)?).map_err(|_wide| Errno::Fault)?;
                let window = memory.window(params.w(0)?, len)?;
                self.random
                    .fill(window)
                    .map_err(|_exhausted| Failure::from(Invariant::Width))
            }
            WasiFunction::SockAccept
            | WasiFunction::SockRecv
            | WasiFunction::SockSend
            | WasiFunction::SockShutdown => Err(Failure::Refused(RefusalReason::Network)),
        }
    }

    /// `fd_write`: standard output and standard error kept as far as their caps, a file written through the overlay.
    fn write(&mut self, memory: &mut GuestMemory<'_>, params: &Params<'_>) -> Result<(), Failure> {
        let fd = params.w(0)?;
        let buffers = memory.buffers(params.w(1)?, params.w(2)?)?;
        let data = memory.gather(&buffers)?;
        let written = match self.files.stream(fd, RIGHTS_FD_WRITE)? {
            Stream::Stdout => {
                self.stdout.keep(&data, self.caps.0)?;
                data.len()
            }
            Stream::Stderr => {
                self.stderr.keep(&data, self.caps.1)?;
                data.len()
            }
            Stream::File => self.files.write(fd, &data)?,
            Stream::Stdin => return Err(Errno::Badf.into()),
        };
        let written = u32::try_from(written).map_err(|_wide| Errno::Inval)?;
        Ok(memory.write_u32(params.w(3)?, written)?)
    }

    /// A set-times request, now being what the realtime clock reads.
    fn times(&self, accessed: u64, modified: u64, flags: u32) -> Result<TimesRequest, Failure> {
        Ok(TimesRequest {
            accessed,
            modified,
            flags,
            now: self.now(CLOCK_REALTIME)?,
        })
    }
}

/// `bytes` with a NUL after them.
fn c_string(bytes: &[u8]) -> Vec<u8> {
    let mut string = bytes.to_vec();
    string.push(0);
    string
}

/// The parameters of one host call, read by position as the import table types them.
pub(crate) struct Params<'call>(&'call [Val]);

impl Params<'_> {
    /// The 32-bit parameter at `at`, as the unsigned number WASI means by it.
    pub(crate) fn w(&self, at: usize) -> Result<u32, Failure> {
        match self.0.get(at) {
            Some(Val::I32(value)) => Ok(value.cast_unsigned()),
            _ => Err(Invariant::Signature.into()),
        }
    }

    /// The 64-bit parameter at `at`, as the unsigned number WASI means by it.
    pub(crate) fn l(&self, at: usize) -> Result<u64, Failure> {
        match self.0.get(at) {
            Some(Val::I64(value)) => Ok(value.cast_unsigned()),
            _ => Err(Invariant::Signature.into()),
        }
    }
}

/// Takes `cost` fuel from the guest, stopping it where there is not that much left, and answers what is left.
fn charge(caller: &mut Caller<'_, Host>, cost: u64) -> Result<u64, HostStop> {
    let left = caller
        .get_fuel()
        .map_err(|_unmetered| HostStop::Broken(Invariant::Width))?;
    let Some(rest) = left.checked_sub(cost) else {
        return Err(HostStop::FuelExhausted);
    };
    caller
        .set_fuel(rest)
        .map_err(|_unmetered| HostStop::Broken(Invariant::Width))?;
    Ok(rest)
}

/// Carries out one call of `function`: the fuel it costs, the work, and its answer or the guest's stop.
///
/// # Errors
/// A stop of the guest, which the runner reads from the host's state rather than from the error.
pub(crate) fn call(
    function: WasiFunction,
    mut caller: Caller<'_, Host>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    match answer(function, &mut caller, &Params(params)) {
        Ok(errno) => {
            if let Some(result) = results.first_mut() {
                *result = Val::I32(i32::from(errno.number()));
            }
            Ok(())
        }
        Err(stop) => {
            let stop = match stop {
                HostStop::FuelExhausted => match caller.set_fuel(0) {
                    Ok(()) => stop,
                    Err(_unmetered) => HostStop::Broken(Invariant::Width),
                },
                HostStop::Exited { .. }
                | HostStop::WatchdogExpired
                | HostStop::Interrupted
                | HostStop::Broken(_) => stop,
            };
            caller.data_mut().stop = Some(stop);
            Err(wasmtime::Error::msg("the sealed host stopped the guest"))
        }
    }
}

/// The error number one call of `function` answers, or the stop it ends the guest with.
fn answer(
    function: WasiFunction,
    caller: &mut Caller<'_, Host>,
    params: &Params<'_>,
) -> Result<Errno, HostStop> {
    if caller.data().interrupt.raised() {
        return Err(HostStop::Interrupted);
    }
    if caller
        .data()
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return Err(HostStop::WatchdogExpired);
    }
    let left = charge(caller, CALL_FUEL)?;
    let spent = caller
        .data()
        .budget
        .checked_sub(left)
        .ok_or(HostStop::Broken(Invariant::Width))?;
    let memory = caller
        .data()
        .memory
        .ok_or(HostStop::Broken(Invariant::Memory))?;
    let allowance = left
        .checked_div(BYTE_FUEL)
        .ok_or(HostStop::Broken(Invariant::Width))?;
    let (bytes, host) = memory.data_and_store_mut(&mut *caller);
    host.spent = spent;
    let mut guest = GuestMemory::new(bytes, allowance);
    let performed = host.perform(function, params, &mut guest);
    let touched = guest.touched();
    let errno = match performed {
        Ok(()) => Errno::Success,
        Err(Failure::Errno(errno)) => errno,
        Err(Failure::Refused(reason)) => {
            host.refuse(function, reason).map_err(HostStop::Broken)?;
            reason.errno()
        }
        Err(Failure::Stop(stop)) => return Err(stop),
    };
    let cost = touched
        .checked_mul(BYTE_FUEL)
        .ok_or(HostStop::FuelExhausted)?;
    charge(caller, cost)?;
    Ok(errno)
}
