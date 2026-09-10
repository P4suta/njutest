<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Trace v1

**Status: the runner's recorder, sinks, and reader are implemented**
(`mjutest_cli::trace`), as is the engine's ([engine/trace](engine/trace.md)).
The vocabulary below is ported from goatest's trace v1.

A recording holds two streams, because two programs record. The runner's is
`mjutest-trace-v1` and the engine's is `rust-mutants-trace-v1`, each naming
its schema in its own `run-start`, so a reader is never left guessing which
program said what.

`mjutest verify --trace[=DIR]` and `mjutest replay ID --trace[=DIR]` record
what a run did while it did it, as JSON Lines under `DIR`, or under
`.mjutest/trace/<UTC timestamp>-<pid>/` by default. `MJUTEST_TRACE=1` or
`MJUTEST_TRACE=DIR` asks for the same without a flag; the variable is read by
the composition root alone. A trace is diagnostic exhaust, never evidence
([ADR 0002](adr/0002-trace-is-not-evidence.md)): it takes no part in a
verdict or in the identity a cached result is keyed on, and a trace that
cannot be written costs a warning rather than the run.

A run that asked for no trace still records into a ring of its last 4096
events in memory, which becomes the `trace.jsonl` of the diagnostics bundle
if the run fails.

## Events

Every line is one JSON object with `seq` (monotonic from 1, in delivery
order), `timestamp` (RFC 3339, UTC), `elapsed_ms` (since the recording
started), `type`, and the type's fields. The stream ends with `run-end`,
which carries `events_emitted` and `events_dropped`.

| Type | Records |
| --- | --- |
| `run-start` | schema, tool versions, run id, run kind, contract |
| `phase-start`, `phase-end` | a phase boundary: name, and on the end its duration. A reader sums these by name, so one name opens one phase: the work inside a stage is named for the work — `baseline-measure` inside `baseline` — and `mjutest trace` says so when a recording opens a name twice |
| `exec` | one command: argv verbatim, dir, environment variable **names**, timeout, exit code, timed-out flag, duration, output digest and preserved-output path |
| `progress` | a progress note as the UI saw it |
| `artifact` | something the run kept (`--keep-temp`) |
| `note` | what has no shape of its own yet |
| `mutant-exec` | one mutant execution: the mutant a person types, the target it ran against, the arguments verbatim, the outcome, how long it took, and whether the machine was given to it, which a run does once when a budget expires |
| `probe-exec` | what the infection layer recorded for one target: `measured` with the count of mutations it saw make a difference, or `not-measured` with no count at all, because a target the guards never recorded carries no facts and none is not zero |
| `route` | one mutant's routing decision: granularity (`all`, `block`, `test`, `discharged`, `unreached`), what widened it (`not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, `touch-incomplete`), the reaching targets in run order, which tests of a target the mutation is put to where a measurement named them, every target a proof removed beside the proof that removed it (`branch-never-taken`, `never-infected`), every measured target that was asked and did not reach the mutation, the run a disposition was read back from, and — when there was a store of earlier answers and it did not answer — why not (`nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`, `key-changed`, `not-passing`, `target-entered`, `nothing-routed`) |
| `run-end` | verdict, accounting, `events_emitted`, `events_dropped` |

An `exec` event carries variable names and never values, and digests the
command's output rather than serializing it; the capture itself reaches only
a sink that preserves it to a file the event then points at.

`events_emitted` and `events_dropped` count what the sink kept and lost
*before* the `run-end` — a recording cannot count the event it is writing.
Anything lost after that shows up to a reader as a missing `run-start` or a
sequence gap, which is why `check` looks for both rather than trusting the
run's own numbers.

`mjutest trace summary [RUN]` reports missing streams, sequence gaps, a
missing `run-end`, and dropped-event counts, and aggregates phase durations,
command classes, routing, and probe measurements. `mjutest trace diff RUN-A
RUN-B` compares event counts and phase durations without replaying either.
