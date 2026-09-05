<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Trace v1

**Status: contract only.** The runner's recorder ships in M2, the engine's
sink in M1 ([engine/trace](engine/trace.md)); the event vocabulary below is
ported from goatest's trace v1 and extended as each phase arrives.

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

Every line is one JSON object with `seq` (monotonic from 1), `ts` (RFC3339),
`type`, and the type's fields. The stream ends with `run-end`, which carries
`events_emitted` and `events_dropped`.

| Type | Records |
| --- | --- |
| `run-start` | run id, contract, mode, tool versions |
| `phase` | a phase boundary: name, start or end, duration |
| `exec` | one command: argv verbatim, dir, environment variable names, timeout, exit code, timed-out flag, duration, output digest and preserved-output path |
| `mutant-exec` | one mutant execution: id, target, args verbatim, outcome, duration |
| `probe-exec` | one probe execution: target, outcome, infected count |
| `route` | one mutant's routing decision: granularity (`block`, `file`, `unreached`), fallback, reaching targets in run order, `discharged` with the proof's name per target, `reused` with provenance |
| `progress` | a progress note as the UI saw it |
| `artifact` | a path the run kept (`--keep-temp`) |
| `run-end` | verdict, accounting, `events_emitted`, `events_dropped` |

`mjutest trace summary [RUN]` reports missing streams, sequence gaps, a
missing `run-end`, and dropped-event counts, and aggregates phase durations,
command classes, routing, and probe measurements. `mjutest trace diff RUN-A
RUN-B` compares event counts and phase durations without replaying either.
