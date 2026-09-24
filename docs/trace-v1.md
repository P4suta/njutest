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
| `fault-exec` | `fault`, `target`, `args`, `outcome`, `duration_ms`, `alone` | one target execution with a fault failing a call; no reader of mutant executions, routes, probes or drift reads one |
| `fault` | `catalog_index`, `id`, `display_id`, `path`, `item`, `position`, `decision` | what one fault site came to, exactly as the report's `faults` holds it |
| `beside` | `mutant`, `fault`, `target`, `failed` | a survivor a target told apart only with the call at its own site failing, exactly as the report's `beside` holds it |
| `beside-run` | `mutant`, `fault`, `target`, `alone`, `with` | one pair of runs of a target behind evidence beside a fault: the fault alone, then the survivor beside it; every pair is recorded, so the evidence is re-derived from these and not read back from itself |
| `crash-exec` | `crash`, `target`, `test`, `stage`, `exit_code`, `outcome`, `left`, `failed` | one run of a test a crash was put to: `crash`, stopped at the call, with the files and directories it `left`; `next`, over what a stop left; `fresh`, in a scratch of its own, with the tests either `failed`; every crash's decision is re-derived from these and its `crash-step` records in order |
| `crash-step` | `crash`, `taken` | one thing a run did about a crash besides running a test, `taken` by `kind`: `route`, the targets it `asked` in order, each with the `tests` that reach the call or `null` where which of them does is not known; `rejected`, the compiler refused it; `tainted`, an earlier stop wrote into the tree so it was not run; `outside`, a stop of it wrote into the tree; a crash's steps and runs together are every step its decision rests on, and the audit refuses a sequence the runner does not make |
| `crash` | `catalog_index`, `id`, `display_id`, `path`, `item`, `position`, `decision` | what one call that writes came to, exactly as the report's `crashes` holds it |
| `probe-exec` | `target`, `outcome`, `infected` | one infection measurement |
| `wire-exchange` | `capability`, `seq`, `during`, `duration_ms`, `read`, `request_bytes`, `response_bytes` | one raw or HTTP exchange; `read` is the closed `wire`/`method`/`path`/`status` object |
| `wire-exec` | `fault`, `capability`, `seq`, `rule`, `answer` | one licensed seam fault and its nested closed decision |
| `sentinel` | `layer`, `mutant`, `expected`, `routed`, `sighted` | one mutant planted for a routing layer before the baseline: `layer` is `reach`, `branch-never-taken`, or `never-infected:` followed by the form of evidence (`is-default`, `is-ok-default`, `is-some-default`, `is-true`, `inert-comparison`); `expected` is `unreached`, the proof that must discharge it, or `kept-for-library` / `kept-for-tests`; `routed` is what the engine decided, as a reader is told; a run ends in `ERROR` (`NJ5009`) after the first one not `sighted` |
| `model` | `mutant`, `answer` | one retained closed model question; `answer` is a nested closed decision and affirmative arms carry their full typed evidence |
| `drift` | `mutant`, `observed` | what the original-code control confirming `mutant`'s kill established about one target's baseline reach; `observed` is the report's closed drift record ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)) |
| `knob` | `target`, `knob`, `standing` | what one control started with one knob put established about one target, the report's closed knob record: `stable`, `passed`, `broke` with the tests that failed, `moved` with the three unions, `uncompared`, `unsettled`, or `not-put` with why |
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
