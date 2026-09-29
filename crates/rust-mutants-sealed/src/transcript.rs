// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one invocation did: how it stopped, what it spent, what it said, what it was refused and what it wrote, under a digest of all of it.

use crate::abi::Errno;
use crate::digest::{Encoder, SealedDigest};
use crate::error::Invariant;
use crate::imports::WasiFunction;

/// How a guest invocation ended; every way is an answer about the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    variant_size_differences,
    reason = "an exit code is four bytes and a trap kind one; boxing the code to even out the ratio would make every stop an allocation"
)]
pub enum SealedStop {
    /// `_start` returned, which wasi-libc does only when `main` returned zero.
    Returned,
    /// The guest called `proc_exit`.
    Exited {
        /// The code it exited with.
        code: u32,
    },
    /// The guest trapped.
    Trapped {
        /// Which trap.
        kind: TrapKind,
    },
    /// The guest spent its whole fuel budget, spinning or waiting for a time the clock never reaches.
    FuelExhausted,
    /// The memory limit refused the guest a growth, and the guest then trapped.
    MemoryExhausted,
    /// A rename put a file at the path the invocation halts at, and the host ended the guest in that call, with nothing after it.
    Halted,
}

/// Every trap wasmtime raises, but running out of fuel and being interrupted, which are stops of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum TrapKind {
    /// An `unreachable` instruction, which is how a Rust guest aborts after a panic.
    Unreachable,
    /// The call stack was exhausted.
    StackOverflow,
    /// A memory access out of bounds.
    MemoryOutOfBounds,
    /// A misaligned atomic access.
    HeapMisaligned,
    /// A table access out of bounds.
    TableOutOfBounds,
    /// An indirect call through a null table entry.
    IndirectCallToNull,
    /// An indirect call whose signature did not match.
    BadSignature,
    /// An integer operation that overflowed.
    IntegerOverflow,
    /// An integer division by zero.
    IntegerDivisionByZero,
    /// A float that could not be converted to an integer.
    BadConversionToInteger,
    /// An atomic wait on a memory that is not shared.
    AtomicWaitNonSharedMemory,
    /// A null reference used.
    NullReference,
    /// An array access out of bounds.
    ArrayOutOfBounds,
    /// An allocation too large to succeed.
    AllocationTooLarge,
    /// A reference cast that failed.
    CastFailure,
    /// A component entered against the reentrance rules.
    CannotEnterComponent,
    /// An async export that produced no result.
    NoAsyncResult,
    /// A suspension to a tag nothing handles.
    UnhandledTag,
    /// A continuation resumed twice.
    ContinuationAlreadyConsumed,
    /// A Pulley opcode that was disabled at compile time.
    DisabledOpcode,
    /// An async event loop that could make no progress.
    AsyncDeadlock,
    /// A component left against its rules.
    CannotLeaveComponent,
    /// A synchronous task that blocked before returning.
    CannotBlockSyncTask,
    /// An invalid `char` lifted.
    InvalidChar,
    /// A string encoding an adapter did not finish.
    DebugAssertStringEncodingFinished,
    /// Code units an adapter expected to be equal.
    DebugAssertEqualCodeUnits,
    /// A pointer an adapter expected to be aligned.
    DebugAssertPointerAligned,
    /// Upper bits an adapter expected to be unset.
    DebugAssertUpperBitsUnset,
    /// A string past the end of its memory.
    StringOutOfBounds,
    /// A list past the end of its memory.
    ListOutOfBounds,
    /// An invalid variant discriminant.
    InvalidDiscriminant,
    /// An unaligned pointer lifted or lowered.
    UnalignedPointer,
    /// `task.cancel` by a task not cancelled.
    TaskCancelNotCancelled,
    /// `task.return` or `task.cancel` called twice.
    TaskCancelOrReturnTwice,
    /// `subtask.cancel` after a terminal status.
    SubtaskCancelAfterTerminal,
    /// An invalid `task.return`.
    TaskReturnInvalid,
    /// A waitable set dropped with waiters in it.
    WaitableSetDropHasWaiters,
    /// A subtask dropped before it resolved.
    SubtaskDropNotResolved,
    /// A thread start function of the wrong type.
    ThreadNewIndirectInvalidType,
    /// A thread start function that is uninitialized.
    ThreadNewIndirectUninitialized,
    /// A backpressure counter that overflowed.
    BackpressureOverflow,
    /// An unsupported callback code.
    UnsupportedCallbackCode,
    /// A thread resumed that was not suspended.
    CannotResumeThread,
    /// Concurrent operations on one future or stream.
    ConcurrentFutureStreamOp,
    /// A reference count that overflowed.
    ReferenceCountOverflow,
    /// A stream operation too large.
    StreamOpTooBig,
    /// A waitable used synchronously while in a set.
    WaitableSyncAndAsync,
    /// An exception that left a component uncaught.
    UncaughtException,
}

/// What a trap of the pinned wasmtime is, among the stops a guest can come to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrapClass {
    /// A trap of the guest's own making.
    Kind(TrapKind),
    /// The guest ran out of fuel.
    OutOfFuel,
    /// The guest was interrupted, which only the watchdog does.
    Interrupt,
}

/// The class of a wasmtime trap, or nothing for a trap a later wasmtime added and this version cannot name.
pub(crate) const fn classify(trap: wasmtime::Trap) -> Option<TrapClass> {
    use wasmtime::Trap;
    let kind = match trap {
        Trap::OutOfFuel => return Some(TrapClass::OutOfFuel),
        Trap::Interrupt => return Some(TrapClass::Interrupt),
        Trap::UnreachableCodeReached => TrapKind::Unreachable,
        Trap::StackOverflow => TrapKind::StackOverflow,
        Trap::MemoryOutOfBounds => TrapKind::MemoryOutOfBounds,
        Trap::HeapMisaligned => TrapKind::HeapMisaligned,
        Trap::TableOutOfBounds => TrapKind::TableOutOfBounds,
        Trap::IndirectCallToNull => TrapKind::IndirectCallToNull,
        Trap::BadSignature => TrapKind::BadSignature,
        Trap::IntegerOverflow => TrapKind::IntegerOverflow,
        Trap::IntegerDivisionByZero => TrapKind::IntegerDivisionByZero,
        Trap::BadConversionToInteger => TrapKind::BadConversionToInteger,
        Trap::AtomicWaitNonSharedMemory => TrapKind::AtomicWaitNonSharedMemory,
        Trap::NullReference => TrapKind::NullReference,
        Trap::ArrayOutOfBounds => TrapKind::ArrayOutOfBounds,
        Trap::AllocationTooLarge => TrapKind::AllocationTooLarge,
        Trap::CastFailure => TrapKind::CastFailure,
        Trap::CannotEnterComponent => TrapKind::CannotEnterComponent,
        Trap::NoAsyncResult => TrapKind::NoAsyncResult,
        Trap::UnhandledTag => TrapKind::UnhandledTag,
        Trap::ContinuationAlreadyConsumed => TrapKind::ContinuationAlreadyConsumed,
        Trap::DisabledOpcode => TrapKind::DisabledOpcode,
        Trap::AsyncDeadlock => TrapKind::AsyncDeadlock,
        Trap::CannotLeaveComponent => TrapKind::CannotLeaveComponent,
        Trap::CannotBlockSyncTask => TrapKind::CannotBlockSyncTask,
        Trap::InvalidChar => TrapKind::InvalidChar,
        Trap::DebugAssertStringEncodingFinished => TrapKind::DebugAssertStringEncodingFinished,
        Trap::DebugAssertEqualCodeUnits => TrapKind::DebugAssertEqualCodeUnits,
        Trap::DebugAssertPointerAligned => TrapKind::DebugAssertPointerAligned,
        Trap::DebugAssertUpperBitsUnset => TrapKind::DebugAssertUpperBitsUnset,
        Trap::StringOutOfBounds => TrapKind::StringOutOfBounds,
        Trap::ListOutOfBounds => TrapKind::ListOutOfBounds,
        Trap::InvalidDiscriminant => TrapKind::InvalidDiscriminant,
        Trap::UnalignedPointer => TrapKind::UnalignedPointer,
        Trap::TaskCancelNotCancelled => TrapKind::TaskCancelNotCancelled,
        Trap::TaskCancelOrReturnTwice => TrapKind::TaskCancelOrReturnTwice,
        Trap::SubtaskCancelAfterTerminal => TrapKind::SubtaskCancelAfterTerminal,
        Trap::TaskReturnInvalid => TrapKind::TaskReturnInvalid,
        Trap::WaitableSetDropHasWaiters => TrapKind::WaitableSetDropHasWaiters,
        Trap::SubtaskDropNotResolved => TrapKind::SubtaskDropNotResolved,
        Trap::ThreadNewIndirectInvalidType => TrapKind::ThreadNewIndirectInvalidType,
        Trap::ThreadNewIndirectUninitialized => TrapKind::ThreadNewIndirectUninitialized,
        Trap::BackpressureOverflow => TrapKind::BackpressureOverflow,
        Trap::UnsupportedCallbackCode => TrapKind::UnsupportedCallbackCode,
        Trap::CannotResumeThread => TrapKind::CannotResumeThread,
        Trap::ConcurrentFutureStreamOp => TrapKind::ConcurrentFutureStreamOp,
        Trap::ReferenceCountOverflow => TrapKind::ReferenceCountOverflow,
        Trap::StreamOpTooBig => TrapKind::StreamOpTooBig,
        Trap::WaitableSyncAndAsync => TrapKind::WaitableSyncAndAsync,
        Trap::UncaughtException => TrapKind::UncaughtException,
        _ => return None,
    };
    Some(TrapClass::Kind(kind))
}

impl TrapKind {
    /// A stable name for the kind, as a transcript's digest spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::StackOverflow => "stack-overflow",
            Self::MemoryOutOfBounds => "memory-out-of-bounds",
            Self::HeapMisaligned => "heap-misaligned",
            Self::TableOutOfBounds => "table-out-of-bounds",
            Self::IndirectCallToNull => "indirect-call-to-null",
            Self::BadSignature => "bad-signature",
            Self::IntegerOverflow => "integer-overflow",
            Self::IntegerDivisionByZero => "integer-division-by-zero",
            Self::BadConversionToInteger => "bad-conversion-to-integer",
            Self::AtomicWaitNonSharedMemory => "atomic-wait-non-shared-memory",
            Self::NullReference => "null-reference",
            Self::ArrayOutOfBounds => "array-out-of-bounds",
            Self::AllocationTooLarge => "allocation-too-large",
            Self::CastFailure => "cast-failure",
            Self::CannotEnterComponent => "cannot-enter-component",
            Self::NoAsyncResult => "no-async-result",
            Self::UnhandledTag => "unhandled-tag",
            Self::ContinuationAlreadyConsumed => "continuation-already-consumed",
            Self::DisabledOpcode => "disabled-opcode",
            Self::AsyncDeadlock => "async-deadlock",
            Self::CannotLeaveComponent => "cannot-leave-component",
            Self::CannotBlockSyncTask => "cannot-block-sync-task",
            Self::InvalidChar => "invalid-char",
            Self::DebugAssertStringEncodingFinished => "debug-assert-string-encoding-finished",
            Self::DebugAssertEqualCodeUnits => "debug-assert-equal-code-units",
            Self::DebugAssertPointerAligned => "debug-assert-pointer-aligned",
            Self::DebugAssertUpperBitsUnset => "debug-assert-upper-bits-unset",
            Self::StringOutOfBounds => "string-out-of-bounds",
            Self::ListOutOfBounds => "list-out-of-bounds",
            Self::InvalidDiscriminant => "invalid-discriminant",
            Self::UnalignedPointer => "unaligned-pointer",
            Self::TaskCancelNotCancelled => "task-cancel-not-cancelled",
            Self::TaskCancelOrReturnTwice => "task-cancel-or-return-twice",
            Self::SubtaskCancelAfterTerminal => "subtask-cancel-after-terminal",
            Self::TaskReturnInvalid => "task-return-invalid",
            Self::WaitableSetDropHasWaiters => "waitable-set-drop-has-waiters",
            Self::SubtaskDropNotResolved => "subtask-drop-not-resolved",
            Self::ThreadNewIndirectInvalidType => "thread-new-indirect-invalid-type",
            Self::ThreadNewIndirectUninitialized => "thread-new-indirect-uninitialized",
            Self::BackpressureOverflow => "backpressure-overflow",
            Self::UnsupportedCallbackCode => "unsupported-callback-code",
            Self::CannotResumeThread => "cannot-resume-thread",
            Self::ConcurrentFutureStreamOp => "concurrent-future-stream-op",
            Self::ReferenceCountOverflow => "reference-count-overflow",
            Self::StreamOpTooBig => "stream-op-too-big",
            Self::WaitableSyncAndAsync => "waitable-sync-and-async",
            Self::UncaughtException => "uncaught-exception",
        }
    }
}

/// Why the host refused a guest's call, each reason answered with one error number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum RefusalReason {
    /// A socket call: a sealed guest has no network.
    Network,
    /// A signal raised: a sealed guest has no signals.
    Signal,
    /// A hard or symbolic link: the overlay holds files and directories only.
    Link,
    /// A path leaving what the directory it is resolved from may reach, or an absolute one that names no place inside it.
    Escape,
    /// A write the overlay has no room left for.
    OverlayFull,
    /// A wait on a clock of the time spent running, which only moves when the guest runs.
    CpuClockWait,
    /// A name its directory holds only in another case, which a case-insensitive file system would answer from that name and the snapshot would not.
    CaseOnly,
}

impl RefusalReason {
    /// The error number the guest is answered with.
    #[must_use]
    pub const fn errno(self) -> Errno {
        match self {
            Self::Network | Self::Link | Self::CpuClockWait => Errno::Notsup,
            Self::Signal => Errno::Nosys,
            Self::Escape | Self::CaseOnly => Errno::Notcapable,
            Self::OverlayFull => Errno::Nospc,
        }
    }

    /// A stable name for the reason, as a transcript's digest spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Signal => "signal",
            Self::Link => "link",
            Self::Escape => "escape",
            Self::OverlayFull => "overlay-full",
            Self::CpuClockWait => "cpu-clock-wait",
            Self::CaseOnly => "case-only",
        }
    }
}

/// How many times the guest was refused one thing by one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusal {
    /// The function refused.
    pub function: WasiFunction,
    /// Why.
    pub reason: RefusalReason,
    /// How many times.
    pub count: u64,
}

/// One output stream as far as its cap, and how many bytes past the cap were counted and not kept.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Captured {
    /// The bytes kept.
    bytes: Vec<u8>,
    /// How many bytes were written past the cap.
    truncated: u64,
}

impl Captured {
    /// Nothing written yet.
    pub(crate) const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            truncated: 0,
        }
    }

    /// Keeps as much of `written` as `cap` allows and counts the rest.
    pub(crate) fn keep(&mut self, written: &[u8], cap: u64) -> Result<(), Invariant> {
        let kept = u64::try_from(self.bytes.len()).map_err(|_wide| Invariant::Width)?;
        let room = cap.saturating_sub(kept);
        let fits = match usize::try_from(room) {
            Ok(room) => room.min(written.len()),
            Err(_wider_than_memory) => written.len(),
        };
        let (keep, past) = written.split_at(fits);
        self.bytes.extend_from_slice(keep);
        let past = u64::try_from(past.len()).map_err(|_wide| Invariant::Width)?;
        self.truncated = self.truncated.checked_add(past).ok_or(Invariant::Width)?;
        Ok(())
    }

    /// The bytes kept.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes were written past the cap, counted and not kept.
    #[must_use]
    pub const fn truncated(&self) -> u64 {
        self.truncated
    }
}

/// What the limits refused the guest's memory and tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Denials {
    /// The sizes in bytes the first refused memory growths asked for, in the order they were asked.
    memory_requests: Vec<u64>,
    /// How many memory growths were refused in all.
    memory: u64,
    /// How many table growths were refused.
    table: u64,
}

/// How many refused memory requests a transcript keeps the size of.
pub(crate) const KEPT_REQUESTS: usize = 16;

impl Denials {
    /// Nothing refused yet.
    pub(crate) const fn new() -> Self {
        Self {
            memory_requests: Vec::new(),
            memory: 0,
            table: 0,
        }
    }

    /// Counts a refused memory growth to `desired` bytes.
    pub(crate) fn memory_refused(&mut self, desired: u64) -> Result<(), Invariant> {
        if self.memory_requests.len() < KEPT_REQUESTS {
            self.memory_requests.push(desired);
        }
        self.memory = self.memory.checked_add(1).ok_or(Invariant::Width)?;
        Ok(())
    }

    /// Counts a refused table growth.
    pub(crate) const fn table_refused(&mut self) -> Result<(), Invariant> {
        match self.table.checked_add(1) {
            Some(table) => {
                self.table = table;
                Ok(())
            }
            None => Err(Invariant::Width),
        }
    }

    /// The sizes in bytes the first refused memory growths asked for.
    #[must_use]
    pub fn memory_requests(&self) -> &[u64] {
        &self.memory_requests
    }

    /// How many memory growths were refused in all.
    #[must_use]
    pub const fn memory(&self) -> u64 {
        self.memory
    }

    /// How many table growths were refused.
    #[must_use]
    pub const fn table(&self) -> u64 {
        self.table
    }
}

/// One path of a preopened tree whose final state differs from its snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OverlayEntry {
    /// The guest path: the preopen's guest path and the path below it.
    pub path: String,
    /// What the path holds at the end of the invocation.
    pub state: OverlayState,
}

/// What a path holds at the end of an invocation, where that differs from its snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OverlayState {
    /// A file with these bytes and times.
    File {
        /// Its bytes.
        contents: Vec<u8>,
        /// Its access time, in nanoseconds since the Unix epoch.
        accessed: u64,
        /// Its modification time, in nanoseconds since the Unix epoch.
        modified: u64,
    },
    /// A directory with these times.
    Directory {
        /// Its access time, in nanoseconds since the Unix epoch.
        accessed: u64,
        /// Its modification time, in nanoseconds since the Unix epoch.
        modified: u64,
    },
    /// Nothing: the snapshot's entry was removed.
    Removed,
}

/// Everything one invocation did, under a digest of its inputs and a digest of itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Transcript {
    /// The digest of everything the invocation was a function of.
    invocation: SealedDigest,
    /// How it ended.
    stop: SealedStop,
    /// The fuel spent.
    fuel_spent: u64,
    /// The largest the linear memory grew, in bytes.
    peak_memory: u64,
    /// Standard output.
    stdout: Captured,
    /// Standard error.
    stderr: Captured,
    /// Every refusal, by function and reason.
    refusals: Vec<Refusal>,
    /// What the limits refused.
    denials: Denials,
    /// How far waits moved the clocks, in nanoseconds.
    waited: u64,
    /// Every path whose final state differs from its snapshot, in path order.
    overlay: Vec<OverlayEntry>,
    /// The digest of everything above.
    digest: SealedDigest,
}

/// The parts of a transcript, before its digest is taken.
#[derive(Debug)]
pub(crate) struct Parts {
    /// The digest of everything the invocation was a function of.
    pub(crate) invocation: SealedDigest,
    /// How it ended.
    pub(crate) stop: SealedStop,
    /// The fuel spent.
    pub(crate) fuel_spent: u64,
    /// The largest the linear memory grew, in bytes.
    pub(crate) peak_memory: u64,
    /// Standard output.
    pub(crate) stdout: Captured,
    /// Standard error.
    pub(crate) stderr: Captured,
    /// Every refusal, by function and reason.
    pub(crate) refusals: Vec<Refusal>,
    /// What the limits refused.
    pub(crate) denials: Denials,
    /// How far waits moved the clocks, in nanoseconds.
    pub(crate) waited: u64,
    /// Every path whose final state differs from its snapshot, in path order.
    pub(crate) overlay: Vec<OverlayEntry>,
}

impl Transcript {
    /// The transcript of `parts`, under the digest of all of them.
    pub(crate) fn seal(parts: Parts) -> Self {
        let mut encoder = Encoder::new("rust-mutants-sealed/transcript/v1");
        encoder.digest(&parts.invocation);
        stop_into(&mut encoder, parts.stop);
        encoder.number(parts.fuel_spent).number(parts.peak_memory);
        for captured in [&parts.stdout, &parts.stderr] {
            encoder.bytes(&captured.bytes).number(captured.truncated);
        }
        encoder.count(parts.refusals.len());
        for refusal in &parts.refusals {
            encoder
                .text(refusal.function.name())
                .text(refusal.reason.name())
                .number(refusal.count);
        }
        encoder.count(parts.denials.memory_requests.len());
        for request in &parts.denials.memory_requests {
            encoder.number(*request);
        }
        encoder
            .number(parts.denials.memory)
            .number(parts.denials.table)
            .number(parts.waited);
        encoder.count(parts.overlay.len());
        for entry in &parts.overlay {
            encoder.text(&entry.path);
            match &entry.state {
                OverlayState::File {
                    contents,
                    accessed,
                    modified,
                } => {
                    encoder
                        .tag(b'F')
                        .bytes(contents)
                        .number(*accessed)
                        .number(*modified);
                }
                OverlayState::Directory { accessed, modified } => {
                    encoder.tag(b'D').number(*accessed).number(*modified);
                }
                OverlayState::Removed => {
                    encoder.tag(b'R');
                }
            }
        }
        let digest = encoder.finish();
        Self {
            invocation: parts.invocation,
            stop: parts.stop,
            fuel_spent: parts.fuel_spent,
            peak_memory: parts.peak_memory,
            stdout: parts.stdout,
            stderr: parts.stderr,
            refusals: parts.refusals,
            denials: parts.denials,
            waited: parts.waited,
            overlay: parts.overlay,
            digest,
        }
    }

    /// The digest of everything the invocation was a function of.
    #[must_use]
    pub const fn invocation(&self) -> &SealedDigest {
        &self.invocation
    }

    /// How the invocation ended.
    #[must_use]
    pub const fn stop(&self) -> SealedStop {
        self.stop
    }

    /// The fuel spent, host calls included.
    #[must_use]
    pub const fn fuel_spent(&self) -> u64 {
        self.fuel_spent
    }

    /// The largest the linear memory grew, in bytes.
    #[must_use]
    pub const fn peak_memory(&self) -> u64 {
        self.peak_memory
    }

    /// Standard output, as far as its cap.
    #[must_use]
    pub const fn stdout(&self) -> &Captured {
        &self.stdout
    }

    /// Standard error, as far as its cap.
    #[must_use]
    pub const fn stderr(&self) -> &Captured {
        &self.stderr
    }

    /// Every refusal, by function and reason.
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    /// What the limits refused.
    #[must_use]
    pub const fn denials(&self) -> &Denials {
        &self.denials
    }

    /// How far waits moved the clocks, in nanoseconds.
    #[must_use]
    pub const fn waited(&self) -> u64 {
        self.waited
    }

    /// Every path whose final state differs from its snapshot, in path order.
    #[must_use]
    pub fn overlay(&self) -> &[OverlayEntry] {
        &self.overlay
    }

    /// The digest of the whole transcript.
    #[must_use]
    pub const fn digest(&self) -> &SealedDigest {
        &self.digest
    }
}

/// Adds a stop to an encoding.
fn stop_into(encoder: &mut Encoder, stop: SealedStop) {
    match stop {
        SealedStop::Returned => {
            encoder.tag(0);
        }
        SealedStop::Exited { code } => {
            encoder.tag(1).number(u64::from(code));
        }
        SealedStop::Trapped { kind } => {
            encoder.tag(2).text(kind.name());
        }
        SealedStop::FuelExhausted => {
            encoder.tag(3);
        }
        SealedStop::MemoryExhausted => {
            encoder.tag(4);
        }
        SealedStop::Halted => {
            encoder.tag(5);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{TrapClass, TrapKind, classify};

    #[test]
    fn every_trap_of_the_pinned_wasmtime_is_a_kind_of_its_own_but_fuel_and_interruption() {
        let traps: Vec<wasmtime::Trap> =
            (0..=u8::MAX).filter_map(wasmtime::Trap::from_u8).collect();
        let unclassified: Vec<&wasmtime::Trap> = traps
            .iter()
            .filter(|trap| classify(**trap).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "traps the pinned wasmtime raises and TrapKind cannot name: {unclassified:?}"
        );
        let classes: Vec<TrapClass> = traps.iter().filter_map(|trap| classify(*trap)).collect();
        let kinds: Vec<TrapKind> = classes
            .iter()
            .filter_map(|class| match class {
                TrapClass::Kind(kind) => Some(*kind),
                TrapClass::OutOfFuel | TrapClass::Interrupt => None,
            })
            .collect();
        let distinct: BTreeSet<TrapKind> = kinds.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            kinds.len(),
            "two traps share a kind: {kinds:?}"
        );
        assert_eq!(
            distinct,
            TrapKind::ALL.into_iter().collect::<BTreeSet<TrapKind>>(),
            "a kind no trap of the pinned wasmtime is"
        );
        let stops: Vec<TrapClass> = classes
            .into_iter()
            .filter(|class| !matches!(class, TrapClass::Kind(_)))
            .collect();
        assert_eq!(stops, [TrapClass::Interrupt, TrapClass::OutOfFuel]);
        let names: BTreeSet<&str> = TrapKind::ALL.iter().map(|kind| kind.name()).collect();
        assert_eq!(
            names.len(),
            TrapKind::ALL.len(),
            "two kinds share a name, and so a transcript digest"
        );
    }
}
