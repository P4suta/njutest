<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Trace v1

**Status: historical.** This page describes stored v1 recordings. Current
runner recordings use [trace v2](trace-v2.md), and current engine recordings
use the separately versioned engine v2 contract. The vocabulary below is
retained for readers of old artifacts.

A recording holds two streams, because two programs record. The runner's is
`njutest-trace-v1` and the engine's is `rust-mutants-trace-v1`, each naming
its schema in its own `run-start`, so a reader is never left guessing which
program said what.

`njutest verify --trace[=DIR]` and `njutest replay ID --trace[=DIR]` record
what a run did while it did it, as JSON Lines under `DIR`, or under
`.njutest/trace/<UTC timestamp>-<pid>/` by default. `NJUTEST_TRACE=1` or
`NJUTEST_TRACE=DIR` asks for the same without a flag; the variable is read by
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

`Fields` is the exact set of keys the record object can serialize, not a
summary. Optional keys are still named, and a flattened closed enum contributes
the union of the keys from all of its variants. A frozen ledger compares the
table in both directions with the v1 vocabulary captured before v2 replaced
the Rust types. It deliberately does not serialize the current types: doing so
would make a replacement rewrite the history it replaced.

| Type | Fields | Records |
| --- | --- | --- |
| `run-start` | `schema`, `njutest`, `rust_mutants`, `run_id`, `run_kind`, `contract` | the schema, tool versions, run id, run kind, and contract |
| `phase-start` | `name`, `duration_ms` | a phase beginning; a reader sums boundaries by name, so one name opens one phase |
| `phase-end` | `name`, `duration_ms` | the matching phase end and its duration. Work inside a stage is named for the work — `baseline-measure` inside `baseline` — and `njutest trace` says so when a recording opens a name twice |
| `exec` | `argv`, `dir`, `env_names`, `timeout_ms`, `exit_code`, `timed_out`, `duration_ms`, `output_bytes`, `output_sha256`, `output_truncated`, `output_path`, `error` | one command: argv verbatim, dir, environment variable **names**, timeout, nullable exit code, timed-out flag, duration, output digest and preserved-output path. The code and flag were independent fields in v1, so the historical shape could spell combinations no process had; a v1 reader preserves that fact instead of silently reading it as v2's tagged `stopped` value |
| `progress` | `message`, `subject`, `done`, `total` | how far a phase has got: `message` as a person watching reads it, and `subject` as a later command takes it |
| `artifact` | `kind`, `path`, `bytes` | something the run kept (`--keep-temp`) |
| `route` | `mutant`, `granularity`, `fallback`, `reaching`, `tests`, `discharged`, `considered`, `reused`, `refused` | one mutant's routing decision: granularity (`all`, `block`, `test`, `discharged`, `unreached`), what widened it (`not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, `touch-incomplete`), the reaching targets in run order, which tests of a target the mutation is put to where a measurement named them, every target a proof removed beside the proof that removed it (`branch-never-taken`, `never-infected`), every measured target that was asked and did not reach the mutation, the run a disposition was read back from, and — when there was a store of earlier answers and it did not answer — why not (`nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`, `key-changed`, `not-passing`, `target-entered`, `nothing-routed`) |
| `mutant-exec` | `mutant`, `target`, `args`, `outcome`, `duration_ms`, `alone` | one mutant execution: the mutant a person types, the target it ran against, the arguments verbatim, the outcome, how long it took, and whether the machine was given to it, which a run does once when a budget expires |
| `probe-exec` | `target`, `outcome`, `infected` | what the infection layer recorded for one target: `measured` with the count of mutations it saw make a difference, or `not-measured` with no count at all, because a target the guards never recorded carries no facts and none is not zero |
| `wire-exchange` | `capability`, `seq`, `during`, `duration_ms`, `wire`, `method`, `path`, `status`, `request_bytes`, `response_bytes` | one exchange that went past a seam the run was watching: the capability, where it fell in the order on that seam, what the run had running where it could tell, how long the round trip took, how much of it was read and what that reading found — `raw` and nothing else, or `http` with the method, the path and the status, never one without the others — and the bytes each way. An audit re-mints the fault identities from these fields alone, which is why the method, the path and the status are here and the bodies are not, and why a line that says `http` and leaves one of the three out is refused rather than read |
| `wire-exec` | `fault`, `capability`, `seq`, `rule`, `decision`, `noticed_by`, `proof` | one fault a recording licensed and the run put to the suite: the fault's identity, the seam and exchange it names, what it asked the seam to do (`truncate-response`, `delay-response`, `drop-connection`, `replay-request`, `stale-response`, `status-server-error`, `status-not-found`), who decided it (`tests`, `unnoticed`, `unreached`, `proved`), and the target that noticed where one did, or the proof that discharged it (`no-body-to-cut`, `already-that-answer`) where the decision was `proved` |
| `note` | `kind`, `detail` | what has no shape of its own yet |
| `run-end` | `verdict`, `accounting`, `error`, `events_emitted`, `events_dropped` | the verdict, accounting, any failure, and the recording's kept and lost event counts |

An `exec` event carries variable names and never values, and digests the
command's output rather than serializing it; the capture itself reaches only
a sink that preserves it to a file the event then points at.

`events_emitted` and `events_dropped` count what the sink kept and lost
*before* the `run-end` — a recording cannot count the event it is writing.
Anything lost after that shows up to a reader as a missing `run-start` or a
sequence gap, which is why `check` looks for both rather than trusting the
run's own numbers.

`njutest trace summary [RUN]` reports missing streams, sequence gaps, a
missing `run-end`, and dropped-event counts, and aggregates phase durations,
command classes, routing, and probe measurements. `njutest trace diff RUN-A
RUN-B` compares event counts and phase durations without replaying either.
