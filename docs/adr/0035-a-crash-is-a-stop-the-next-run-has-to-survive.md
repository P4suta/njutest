<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0035 — A crash is a stop the next run has to survive

## Status

Proposed, 2026-09-24.
The dimension `durable` of the assurance matrix ([ADR 0033](0033-every-dimension-or-a-hole.md)).
Amended by [ADR 0046](0046-a-verdict-is-what-a-sealed-run-observed.md): a sealed crash is confirmed in one round, the next instance starting from the crashed one's overlay.
Implemented natively by `crash-after-write`, the runtime's crash notice, `njutest::assure::crashes` and the proofaudit `crashes` layer.
What holds, as the tree stands on 2026-09-29:

| Decision | Held by |
| --- | --- |
| 1, a call that writes | discovery's `crash-after-write`; a call whose value is a future is refused by the compiler through the runtime's `Written` and is `not-put`, in `touch_runtime::a_stop_after_a_call_whose_value_is_a_future_is_one_the_compiler_refuses` |
| 2, the stop, the notice and the child | the runtime's `crashed_after` and notice, the engine's `Stop`; a notice published by a process the test started, with the test's own process not stopped, is `undecided` in the runner's `Noticed` and in the audit's re-decision, in `assure::crashes::tests::a_notice_published_by_a_process_the_test_started_is_a_stop_elsewhere` and `crashes::a_stop_in_a_child_the_test_started_leaves_the_crash_undecided_whatever_the_parent_did`; one run per nonce in `crashes::a_nonce_is_one_run_s_and_a_second_run_carrying_it_is_refused` |
| 3, the next run and its audit | `njutest::assure::crashes` and the proofaudit `crashes` layer, over the steps the recording keeps |

What stands between it and acceptance:

- The amendment: every crash runs natively in three rounds; nothing starts an instance from another's overlay, a crash record does not say it was sealed, and the sealed build the crash session makes is read by nothing.
- Decision 2: the stop is `std::process::exit`, which flushes standard output, runs the C runtime's exit handlers and, on glibc, the calling thread's thread-local destructors, so "no flush after it" does not hold for state kept there, and no test holds that nothing after the call is written.
  The standard library has no safe exit that skips them, and an `unsafe` call to `_exit` in the generated runtime would stop every crate that forbids `unsafe_code` from compiling; in a sealed run the host can take the overlay at the notice, which is the amendment's work.
- Decision 2: no toolchain run puts a crash a child process reaches; the classification is held by unit tests of the runner and the audit.
- Decision 3: an entry of the crashed scratch whose name is not UTF-8 ends the phase rather than leaving the crash `undecided`, because the engine's walk of what a stop left refuses the entry and the runner does not read that refusal as a run that could not be read; a crash that wrote only to a home the run was given reads as `unshared`.
- Decision 5: a tree with no write call is known to have nothing to ask only after its crash build and baseline, and a baseline that fails there makes the column `unmeasured`.
- The runner and the audit are never run together on a real recording; `copy`, `remove_file`, `sync_all`, `sync_data`, `set_len` and `flush` are never sites in a test.

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
   A call whose value is a future has written nothing when it returns, so a stop after it would be a stop before the write: the compiler refuses it, and the crash is not put.
   Before it stops, the runtime publishes a notice naming this execution's nonce, the catalog and the crash, outside the scratch the test sees; the exit status alone is something a test can return, so a stop is the status and the notice together, and the engine keeps its own files beside the scratch rather than in it, so what the scratch holds is only what the test left.
   Only the engine says a run stopped: the value it hands back after verifying the notice is the one thing a stop can be recorded from, so a runner cannot record one it did not see.
   The recording keeps the evidence rather than the verdict: each crashed run's record carries what the engine issued it — the full mutation, the catalog, the nonce — and the notice it read back, and the audit decides the stop again from exactly that, holding the record's `noticed` to it, the mutation to the report's site, and every nonce to one run.
   A child the test starts inherits the notice's path and nonce, and one that runs the crash's call itself publishes the notice and ends with the stop's status; its parent then fails for having lost the child, the process as a whole does not end with 93, and the run is not a stop — which leaves the crash `undecided`, as it should, since the stop happened in a process the next run's test did not make.

3. **The next run is the observer, in the state the crash left.** A target whose process ended at the crash is run again with nothing active in the same scratch directory — the same temporary directory, holding exactly what the crashed process left — and the paths the crash left there are named.
   It is `restarted`, with those paths, where that run passes; `corrupt` where it fails with the stopped test among its failures, and three rounds each confirm it — a run in a fresh scratch passes, and another crash at the site leaves something and fails the next run again with the same failures — so a test that fails half its runs by itself passes for corrupt about once in 128; `unshared` where the crashed process left nothing in its scratch, so the next run could not have read anything of it; `unreached` where no reaching test stopped at the site; and `undecided` where a bound expired or a run could not be read.
   `corrupt` is a `corrupt-after-crash` finding and a `DEFECT`: the program cannot start over what it wrote.
   Every step a decision rests on is recorded: the route with the tests it asks in order, each run, a refusal, a crash left alone because an earlier stop wrote into the tree, and a stop that did.
   The audit decides each crash again from those steps alone and holds the report to exactly that decision, its counts and its findings; a step missing, added after the decision, or out of order is refused rather than read around.
   `unshared` and `undecided` are holes, stated as `not-measured` findings as a fault's are.

4. **What it speaks about is a restart over changed state, and nothing more.** `restarted` says the next run passed over the paths the crash left, not that it read them: a test that keeps its state under a name it chooses afresh every run reads nothing its predecessor left.
   And a process that stops keeps what it wrote in the operating system's cache, so a crash here is a process stopping, not the power failing.
   The column says both: it does not speak about whether the next run read what the crash left, nor about writes the system had not flushed to disk.

5. **It is opt-in, and `whole-v1` turns it on.** `[durability] crash = true` or `--crashes` asks for it; a tree with no write call in a measured file has nothing to ask.

## Consequences

- Every write call a test reaches costs two executions, eleven where it is corrupt.
- A torn write, a missing rename-into-place, a reader that does not tolerate a partial file are each named at the call that tears them.
- The durable column of the matrix is measured, and `whole-v1` becomes satisfiable once schedules are.
