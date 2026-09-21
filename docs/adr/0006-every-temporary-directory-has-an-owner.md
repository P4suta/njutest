<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0006 — Every temporary directory has an owner

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0006 (accepted 2026-09-04) and shared with the `tempowner` convention of every engine in the family.

## Context

A run writes far more outside the repository than inside it: the engine's snapshot, its probe tree, its scratch; the runner's per-run build directory,
a scratch per baseline round, a tree per validated candidate.
Every one of those directories used to be made directly under the temporary root and removed by a deferred call in the process that made it.
A SIGKILL, an out-of-memory kill, or a closed terminal ends the process between the copy and the removal, and what is left is a full copy of somebody's project that nothing will ever delete, under a name that says nothing about who made it or whether anybody is still using it.

Two questions must be answerable about a directory found in a temporary root: *who made this*, and *is anybody still using it*.

## Decision

1. **One scratch directory per run, and everything below it.** A run makes `njutest-run-*` under the configured temporary root before it writes anything: `build/` for per-run Cargo isolation, `baseline-*` per round,
   `candidate-*` per validated candidate, `control-fuzz-*` for a fuzzing original control.
2. **A run that cannot make or claim one still runs**, under the temporary root and the names the sweep knows.
3. **The lock is the liveness signal, not the pid.** A claimed directory holds `owner.lock`, an exclusive advisory lock held open for the whole run.
   A lock that can be taken means its holder is gone; a pid wraps.
4. **The marker is for people, and for one bit.** `owner.json` is a `njutest-temp-owner-v1` document naming the run, the process, the start time, the repository, and `kept`.
   The sweep reads only `kept`.
5. **An unowned directory is judged by age, and 24 hours is the number.**
6. **Sweeping is what a run does before it writes, and what `cache gc` does on demand.** The engine sweeps its own prefixes in `Workspace::open`; the runner reports that rather than duplicating it.
7. **A keep is recorded where it outlives the run**, in `.njutest/kept-temp-v1.json`, so a successful untraced run still accounts for what it left behind.
8. **The ledger names a directory; the directory says whether it may be removed.** A recursive delete is not something a path in an editable file may authorize; the marker must vouch for it.
9. **`[cache] ttl` bounds a keep, and nothing else does.**
10. **None of this can fail a run.**

## Consequences

- The lock is released before the directory is removed, everywhere; on Windows an open handle inside a directory is what makes removal fail.
- Two runs on one machine never contend: each holds its own lock, and a sweep that meets a live one counts it and moves on.
- The engine's and the runner's sweeps leave each other's directories alone by prefix and would agree about any directory they both looked at.
- A kept directory whose marker was removed waits for a person; that is the safe side of the trade.
