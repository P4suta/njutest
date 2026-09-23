<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Trace v1

Published schema: [`schema/njutest-trace-v1.json`](../schema/njutest-trace-v1.json).

**Status: implemented.** Current runner recordings identify themselves as `njutest-trace-v1`.
Historical v1 recordings remain documented separately and are never claimed to have this shape.
A trace is diagnostic exhaust, never evidence, and cannot establish a verdict or cache identity.
An explicitly requested trace is nevertheless a command requirement: directory setup fails before verification starts, and a durable write or final-sync loss fails the command rather than being hidden by the in-memory diagnostic ring.

Every JSON Lines event is the closed envelope `seq`, `timestamp`,
`elapsed_ms`, and `payload`.
`payload` is a closed tagged object whose `type` selects one record.
Nesting makes an envelope/payload field collision unrepresentable.
The stream begins with `run-start` and ends with exactly one `run-end`.
Sequence gaps, a missing end, and dropped events are reported.

`Fields` below is the exact serialized key set, including optional keys.
A test serializes closed specimens and compares this table in both directions.

| Type | Fields | Records |
| --- | --- | --- |
| `run-start` | `schema`, `njutest`, `rust_mutants`, `run_id`, `run_kind`, `contract` | current schema and run identity |
| `phase-start` | `name`, `duration_ms` | one phase beginning |
| `phase-end` | `name`, `duration_ms` | the matching phase end and duration |
| `exec` | `argv`, `dir`, `env_names`, `timeout_ms`, `stopped`, `duration_ms`, `output_bytes`, `output_sha256`, `output_truncated`, `output_path`, `error` | one command and its single tagged termination; environment names, never values |
| `progress` | `message`, `subject`, `done`, `total` | bounded progress through a phase |
| `artifact` | `kind`, `path`, `bytes` | a retained file or directory |
| `route` | `mutant`, `granularity`, `fallback`, `reaching`, `tests`, `discharged`, `considered`, `reused`, `refused` | one mutation route: `all`, `block`, `test`, `discharged`, or `unreached`; fallback `not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, or `touch-incomplete`; reuse refusal `nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`, `key-changed`, `not-passing`, `target-entered`, or `nothing-routed` |
| `mutant-exec` | `mutant`, `target`, `args`, `outcome`, `step_boundary`, `duration_ms`, `alone` | one target execution; the checked boundary exists only for `step_limit_reached` |
| `probe-exec` | `target`, `outcome`, `infected` | one infection measurement |
| `wire-exchange` | `capability`, `seq`, `during`, `duration_ms`, `read`, `request_bytes`, `response_bytes` | one raw or HTTP exchange; `read` is the closed `wire`/`method`/`path`/`status` object |
| `wire-exec` | `fault`, `capability`, `seq`, `rule`, `answer` | one licensed seam fault and its nested closed decision |
| `model` | `mutant`, `answer` | one retained closed model question; `answer` is a nested closed decision and affirmative arms carry their full typed evidence |
| `note` | `kind`, `detail` | a named diagnostic with no richer event type |
| `run-end` | `verdict`, `accounting`, `error`, `events_emitted`, `events_dropped` | the sole terminal event, emitted only after report persistence and cleanup succeed |

`run-end.accounting` is an explicit nullable field.
A complete run carries the final cross-build target and mutant counts plus `soundness_by_build`: one closed `{ build, accounting }` row for every configured build in request order.
Soundness is never selected from the first build or collapsed by a maximum.
A shard or a run that failed before it could complete carries `accounting: null`; absence of the key is malformed rather than another spelling of null.

`exec.stopped` is the engine's tagged termination union.
It cannot claim two terminal causes.
`mutant-exec.step_boundary` is the checked `limit` and exact `observed = limit + 1` pair from the nonce-correlated runtime notice; a bare exit code cannot manufacture it.

## Configured-build engine recordings

The runner recording owns a `builds/` directory whenever `--trace` is explicit.
Each requested build owns exactly one exclusive `builds/<zero-padded ordinal>/engine/` directory.
Names are report data, never path components.
Every engine `run-start.context` binds the final runner run id, its build-internal run id, ordinal, configured name, and canonical `BuildSelection` digest.
It binds the seven Cargo options selected by the configuration, while toolchain and resolved-host evidence remain separate.
A reader can therefore reject a missing namespace, a swapped trace, or a trace produced under different Cargo features, profile,
or target without trusting directory names or operator attention.
