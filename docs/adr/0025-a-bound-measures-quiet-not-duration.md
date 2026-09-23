<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0025 — A bound measures quiet, not duration

## Status

Accepted, 2026-09-23.
Bounds what `[mutation] timeout` means for an execution that counts its steps.

## Context

A mutation can stop a program ending, and two things can stop it: the step allowance, a count every machine agrees on, and the timeout, a clock.
The allowance is machine-independent as a decision.
Whether it is reached before the clock is not: a take costs about half a microsecond on one machine and cost 7.3ms on another, so no choice of number makes the count reliably arrive first.

While the timeout bounded the whole execution, a busy machine could stop a mutation the allowance was about to catch, and report `waited` where a quiet machine reported `step_limit_reached`.
The same held for a test that was merely slow: it finished on one machine and was cut short on another.
Both are a verdict resting on how fast the machine was, which ADR 0023 forbids.

Two earlier directions were measured and dropped.
Tuning the allowance cannot work, because the number is not what varies.
Deriving the bound from the baseline's count cannot either, for the same reason: it is still a count racing a clock.

## Decision

For an execution that counts its steps, `timeout` is how long it may go **without raising the count**.
The runner reads the step state the process rewrites at every boundary, and any change in its content starts the window again.

- A process that spins through instrumented source keeps raising the count and is ended by the allowance, at the same count everywhere.
- A process that blocks raises nothing, and the window ends it as `stalled`, which is `waited`.
- A process that is merely slow keeps raising the count and finishes.

A ceiling of ten windows stays over the whole execution, because a process can keep moving forever more slowly than the allowance can end it; one that reaches the ceiling is `timed-out` with the count it had raised.
An execution that counts nothing — every baseline, every control, every run with `steps = 0` — keeps `timeout` as a bound on the whole of it.

## Consequences

Neither decision about a counting execution rests on the machine's speed; both rest on whether the process was moving, which is a property of the program.

The window is still a duration.
It has to be long enough that a machine pausing a process does not read as quiet, so the dependence on the machine is not zero; it bounds scheduling noise rather than the work, which is a far weaker dependence than the one it replaces.

A content change is the signal rather than the parsed count, so a read torn by a write still counts as the write it is, and a process rewriting the same state is not taken for moving.
A failed read is never a change: a process that is not writing cannot cause one.
On Windows the runtime's lock over the state is mandatory, so a read that lands inside a take is refused; the runner tries it again for as long as a take holds the lock, so a process taking steps back to back is not read as quiet.
The file is the supervised process's to replace, so it is read without following a link, without blocking on a pipe, as a regular file of at most 16 KiB, like the notice.

The ceiling has a cost.
A mutation that spins more slowly than the allowance can end within ten windows — a take costing more than a tenth of a window over the allowance — now runs to the ceiling where it used to run to one window, and a timed-out execution is asked once more alone.
With the thirty-second floor that is five minutes, twice.
It is paid only where the count was going to lose the race anyway, which is exactly where the old bound was deciding from the machine's speed.
The trace records the window beside the ceiling, as `quiet_ms` beside `timeout_ms`, so an execution that stalled says which of the two ended it.

`Stopped` gains `stalled`, beside `timed-out`, in both trace formats.
A reader that needs to know whether the clock raced the count reads `timed-out` with a count above zero; a reader that needs to know whether anything was moving reads `stalled`.

Things this does not reach, recorded as decided rather than open: a mutation that is slower but terminating is the clock's and has to be; `delay-response` is a fault that is itself a duration; and nondeterminism inside the project's own tests is outside anything the engine can remove.
