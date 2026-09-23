<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0028 — A process that loses the environment says so

## Status

Accepted, 2026-09-24.
Implemented by the `watched` check of the generated runtime, `orphan` and the baseline of `session::verify` in rust-mutants, and `uncontrolled-child`.

## Context

A run reaches every process of an instrumented tree through its environment.
The variable naming the active mutant, the one naming the touch log, and the catalog they are about are all set on the test process and inherited by whatever it starts.
A test that starts a child with `env_clear()` — to check a binary behaves the same in any environment — starts a process none of them reach.
No mutant can be active in it, and nothing records what it entered.

A review found the consequence on a fixture: every mutant of a function only such a child ran was recorded as unreached, which claims no test reached it, and the same shape recorded survivors where a test also reached it in-process.
Both are claims about a measurement that was never made.
The select layer inherits the same hole: a target whose child entered an item it never entered itself would be skipped for a change to it.

## Decision

1. **The tree knows where it reports.** The generated runtime of every instrumented file carries, as a constant, the absolute path of a directory beside the run's copy of the tree, which no other run shares.
   Every process a run starts carries that path in `RUST_MUTANTS_WATCHED`; `Workspace::open` sets it on the environment every process inherits, replacing any value an outer run left.

2. **A process that does not carry it says so.** On its first guard or entry marker, a process whose `RUST_MUTANTS_WATCHED` is missing or names another directory creates `orphan-<pid>-<parent pid>` in the directory, once.
   It is std-only and needs no `unsafe`, so it builds in a crate that forbids it; the parent is `std::os::unix::process::parent_id`, and zero where the platform does not say.
   A process that cannot write the file exits with the touch-unavailable code rather than run on unseen.

3. **The baseline attributes it.** Baselines run one target at a time, so the directory is cleared before each and read after it: anything there was left by that target's processes.
   Such a target is `uncontrolled-child`: its touch record is not gathered, so it stays in every route, and a selection finds nothing measured about it and runs it.
   A directory that cannot be read counts as one that holds an orphan.

4. **A survival from it is not one.** An execution against an `uncontrolled-child` target that comes back `survived` is recorded `inconclusive`: every test of it passing says nothing about a mutant that may have lived only in a process it could not reach.
   A kill stays a kill, since the confirming control ran in the parent.

## Consequences

- A suite that clears its children's environment loses precision on the targets that do, and no longer loses soundness.
- Only the baseline is watched.
  A process that loses the environment under one mutant and not on the baseline — a mutant that makes a test take a path that spawns — is not yet attributed; mapping an orphan found during the mutation phase back to its execution by its parent's id, and to every execution in flight where that fails, is the next change.
- Reaching such a child at all, rather than only noticing it, needs a channel `env_clear()` does not remove: an inherited descriptor opened before the test starts, which the engine can pass without `unsafe` only once it has a way to mark one inheritable.
