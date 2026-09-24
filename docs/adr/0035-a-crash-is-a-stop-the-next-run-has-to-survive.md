<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0035 — A crash is a stop the next run has to survive

## Status

Proposed, 2026-09-24.
The dimension `durable` of the assurance matrix ([ADR 0033](0033-every-dimension-or-a-hole.md)).

## Context

A program that keeps state on disk makes a promise no test of a single run can check: that whatever it wrote before it stopped, the next run can read.
A suite exercises a write that completes and a read that follows it in the same process.
It never exercises a process that stops between two writes, which is the one case the promise is about.
A torn file — a header written and a body not — passes every test that writes and reads in one go.

Nothing here needs a syntax of ours ([ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md)).
The suite already is the check: a test that reads what the previous run left is the program's own recovery path, run as the program runs it.

## Decision

1. **A crash site is a call that writes in a measured file.** Discovery names each call to `std::fs::write`, `rename`, `copy`, `remove_file`, `create_dir_all` or `File::create`, and each call of the methods `write_all`, `sync_all`, `sync_data`, `set_len` or `flush`, as the rule `crash-after-write` of the family `durable`.
   Like faults, no tier chooses it, no proof removes a target from it, and no guard of it is carried into another's branch; `Perturbs::Crash` says so once.

2. **A crash lets the call finish and then stops the process.** Under the crash, the call is evaluated and the process ends at once with its own exit status, with no destructor, flush or unwinding after it: what the call wrote is on disk, and nothing after it is.
   Stopping after the call rather than before it is the case that tears state: before it, the write never happened, which a program already has to be ready for.
   Before it stops, the runtime publishes a notice naming this execution's nonce, the catalog and the crash, outside the scratch the test sees; the exit status alone is something a test can return, so a stop is the status and the notice together, and the engine keeps its own files beside the scratch rather than in it, so what the scratch holds is only what the test left.

3. **The next run is the observer, in the state the crash left.** A target whose process ended at the crash is run again with nothing active in the same scratch directory — the same temporary directory, holding exactly what the crashed process left — and the paths the crash left there are named.
   It is `restarted`, with those paths, where that run passes; `corrupt` where it fails, a run of the same target in a fresh scratch passes, and a second crash at the site leaves something and fails the next run again with the same failures, the stopped test among them; `unshared` where the crashed process left nothing in its scratch, so the next run could not have read anything of it; `unreached` where no reaching test stopped at the site; and `undecided` where a bound expired or a run could not be read.
   `corrupt` is a `corrupt-after-crash` finding and a `DEFECT`: the program cannot start over what it wrote.
   Every step a decision rests on is recorded: the route with the tests it asks in order, each run, a refusal, a crash left alone because an earlier stop wrote into the tree, and a stop that did.
   The audit decides each crash again from those steps alone and holds the report to exactly that decision, its counts and its findings; a step missing, added after the decision, or out of order is refused rather than read around.
   `unshared` and `undecided` are holes, stated as `not-measured` findings as a fault's are.

4. **What it speaks about is a restart over changed state, and nothing more.** `restarted` says the next run passed over the paths the crash left, not that it read them: a test that keeps its state under a name it chooses afresh every run reads nothing its predecessor left.
   And a process that stops keeps what it wrote in the operating system's cache, so a crash here is a process stopping, not the power failing.
   The column says both: it does not speak about whether the next run read what the crash left, nor about writes the system had not flushed to disk.

5. **It is opt-in, and `whole-v1` turns it on.** `[durability] crash = true` or `--crashes` asks for it; a tree with no write call in a measured file has nothing to ask.

## Consequences

- Every write call a test reaches costs three executions, four where it is corrupt.
- A torn write, a missing rename-into-place, a reader that does not tolerate a partial file are each named at the call that tears them.
- The durable column of the matrix is measured, and `whole-v1` becomes satisfiable once schedules are.
