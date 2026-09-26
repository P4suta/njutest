<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0039 — A step is spent in memory

## Status

Accepted, 2026-09-25.

## Context

The step allowance is one count shared by every instrumented crate of a process and every process it starts, so it lives in a file, and every function entry and loop turn of a mutated run read that file under a lock and wrote it back.
A suite performs millions of boundaries.
An adopter on Windows measured a mutant killed in 233 ms with steps off time out with them on: one test target took 112 seconds instead of 1.5, which the clock then reported as `waited`, a timing artefact deciding a verdict ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md)).
Opening the file once per copy took the round trip from 4 ms to 44 µs; the count was still what cost.

## Decision

1. **A copy reserves and spends in memory.** At a boundary it needs to charge, a copy takes the lock, applies the proven `step_transition` for that boundary, and then for as many more as a reservation grants — a sixteenth of what is left of the allowance, at least one and at most 4 096 — writes the state once, and spends the grant from an atomic counter.
   A reservation never crosses the allowance: the boundary past it is decided alone, under the lock, and publishes the notice exactly as before.
2. **A dormant copy asks rarely.** A copy that last saw the shared phase dormant asks again every 256 boundaries, since another copy's activation can only be learned from the file; the copy that owns the selected guard activates synchronously, and a copy that knows it is active never asks again to activate.
3. **What is exact is said.** The allowance is exact for the copy that activates the mutation.
   With several copies spending at once a stop can come early by what the others still hold, at most one reservation each, and a dormant copy charges nothing for up to 256 boundaries after another activates; both are bounded counts, not durations.
4. **Counting stays per boundary**, because a count lost at process exit would lower the floor the allowance is sized from.

## Consequences

- A dormant boundary costs 30 ns and an active one 90 ns on a loaded Mac, against 3.2 µs and 35 µs for a round trip each; the Windows figures are in the pull request.
- The state file changes only at a reservation, and a reservation can hold 4 096 boundaries, which a slow test spends over far longer than one quiet window ([ADR 0026](0026-a-bound-measures-quiet-not-duration.md)).
  The first form of this decision said the watcher still saw progress while a copy spent; that held only while a reservation was spent inside a window, which nothing stated, and a test moving every fifty milliseconds under a large allowance was stopped as `stalled`.
  So the contract is now one the runner states: it tells the process it watches a beat, a quarter of its window, and a copy spending a reservation rewrites a beat file once that long has passed since the last, which the runner watches beside the state.
  The beat costs one clock read per boundary spent in memory and a file write per beat, and it leaves the proven transition and the count untouched.
- A child made by fork without exec inherits the parent's unspent reservation; it spends at most that many before asking.
