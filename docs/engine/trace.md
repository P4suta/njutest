<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Engine trace

**Status: implemented.** `rust_mutants::trace` holds the `Recorder`, the `Sink` enum with best-effort `MemorySink`/`ChannelSink`, durable `DirSink`,
and a closed `RequiredSink` that keeps the durable authority distinct from its observers; `read_events` and `check` for reading a stream back, and `summary` for reading one as numbers.
The command line records under `--trace[=DIR]` and reads a recording back with `rust-mutants trace summary|check|diff`.

The rules are those of [ADR 0002](../adr/0002-trace-is-not-evidence.md): a recording is never a claim and is honest about what it dropped.
When no trace was requested, an in-memory progress observer remains best effort.
An explicit `--trace` is a durability request: failure to claim its directory refuses the command before execution, and any later loss or final-sync failure makes finalization fail.
An observer channel cannot hide a failure of that durable authority.

## Where a recording goes

`--trace` names a directory, and without one the engine picks the place a reader would look.
A run records beside its own report, under `<reports.directory>/<run id>/trace/`; every other command records under `<reports.directory>/traces/<run id>-<command>/`, which is outside what a snapshot copies.
The run is named before the workspace is opened, so the opening itself is recorded.
`--trace --no-report` still makes the run's directory.
Both places are pruned by `reports.keep`, counted separately, so a recording never costs a stored report its place.

A recording owns its directory: a directory that is already there is refused rather than appended to, which is what keeps two runs from writing one stream.

When `njutest` owns the run, each configured build has a distinct engine recording under `builds/<zero-padded ordinal>/engine/`.
A raw configuration name never becomes a path.
All namespaces are claimed before the first build starts, so a requested multibuild trace cannot degrade into a partial set.

## What a line is

Every line is one JSON object: `seq` (monotonic from 1, delivery order is sequence order however many threads record), `timestamp` (RFC 3339, UTC),
`elapsed_ms`, and a closed `payload`.
The payload's `type` selects the record under a key named after that type.
The first event is `run-start` with `schema: "rust-mutants-trace-v1"`,
the engine version, and a closed `context`.
A standalone context binds the canonical run id and Cargo `BuildSelection` digest.
The selection covers the seven Cargo options controlled by the engine; toolchain and resolved host inputs are separate evidence rather than being overclaimed by this digest.
An `njutest` context binds the final run id, build-internal run id, zero-based ordinal,
configured name, and that same canonical build digest.
The last event is `run-end` with `outcome`,
`events_emitted`, and `events_dropped`, where a sink that counts its own drops (a full ring, a failed write) is the authority and the recorder's observed failures fill in otherwise.
The outcome is the word the command's exit code is named after: `detected` when everything the run executed was noticed and there is nothing to report, `found` when it reported a finding of any kind (a survivor, a stale claim, something it could not decide), `failed` when the command itself could not finish, and `interrupted` when it was stopped.

An `exec` record carries environment variable *names* only (the recorder strips `=value`), and the output as `output_bytes` plus `output_sha256`; a `DirSink` preserves the capture beside the stream under `output/<seq>.txt`,
cut at 1 MiB with a `...` marker, and records `output_path` and `output_truncated`.
The disabled trace is `Recorder::disabled()`; call sites record unconditionally.
`Recorder::new` takes the clock as an argument, which is how the goldens in `crates/rust-mutants/tests/testdata/trace/` freeze the shape, and every line is checked against [`schema/rust-mutants-trace-v1.json`](../../schema/rust-mutants-trace-v1.json).

## The types

`Fields` is the exact set of keys the record object can serialize, not a summary.
Optional keys are still named.
Tests serialize non-empty specimens and compare both directions, so adding, removing, or repeating a key cannot leave this table green.

| Type | Fields | Records |
| --- | --- | --- |
| `run-start` | `schema`, `engine`, `context` | the schema and engine version plus a closed standalone or `njutest` build binding |
| `phase-start` | `name`, `duration_ms` | a phase beginning |
| `phase-end` | `name`, `duration_ms` | the matching phase end and its duration |
| `open` | `root`, `snapshot_dir`, `stable_dir`, `sweep` | the root, snapshot path and whether its stable name was available, and the sweep result |
| `snapshot` | `source_root`, `dir`, `files`, `bytes`, `workspace_digest`, `duration_ms`, `error` | the tree copied, its destination, files, bytes, digest, duration, and any refusal |
| `exec` | `argv`, `dir`, `env_names`, `timeout_ms`, `quiet_ms`, `stopped`, `duration_ms`, `output_bytes`, `output_sha256`, `output_truncated`, `output_path`, `error` | every process: argv verbatim, dir, environment variable names, timeout (the ceiling, for a process watched for progress), the quiet window it was watched for or null, and one closed stop reason — not started, its own code/signal/unknown exit, timed out, stalled with no step raised for a whole quiet window, cancelled, wait failed, a verified step-limit notice, or a failed step protocol — plus duration, output digest and preserved output. A process cannot be both timed out and exited |
| `discover-file` | `path`, `candidates`, `sites`, `skips` | per file: candidates found, and every site with its form (`C`, `E`, `S`) or its skip reason |
| `instrument` | `path`, `guards`, `module`, `lines_before`, `lines_after` | per file: guards placed, the runtime module's name, and the line count before and after, which must be equal |
| `validate-round` | `round`, `condemned`, `success`, `written`, `attributed`, `unattributed` | per round: how many were condemned going in, how many files it had to write again, whether the tree compiled, which mutant each error was attributed to with the compiler's first line, and the errors no branch accounts for |
| `bisect` | `suspects`, `offenders`, `attempts`, `diagnosed` | per isolation: how many suspects, which offenders it named, how many compilations it cost, and how many it could put the compiler's own words to |
| `build` | `targets`, `details` | the test binaries the build produced, each with its kind, whether it carries the libtest harness, and what a run could not establish about it |
| `verify` | `target`, `outcome`, `tests_run`, `duration_ms`, `remembered`, `retried` | per target: what the suite established with nothing active, how many tests ran, how long the baseline took, whether that exact passing measurement was remembered, and whether it retried |
| `touch` | `target`, `measured`, `passed`, `summary`, `tests`, `sites`, `loose`, `infected`, `entered`, `reached_sites`, `entered_bodies`, `infected_sites`, `entered_items` | per target and per whole run of it with nothing active — `baseline`, or a `control` run to confirm a kill: the tests that run passed, what its own summary said in the protocol it answered in (`libtest` with its count, `custom`, `unanswered`, or `remembered` for a baseline from an earlier session), how many of its tests reached a mutation, how many distinct mutations anything of it reached, how many were reached where nothing named a test, how many it saw a guard's two branches differ over, how many distinct items anything of it entered the body of, and the four unions themselves (sites, bodies, infected, entered items), which is what `xtask proofaudit` compares between the two runs ([ADR 0025](../adr/0025-a-reach-that-moves-is-not-a-measurement.md)) |
| `witness` | `index`, `witnesses`, `checked`, `diagnostic` | per candidate: the witness placed, whether it checked, and the diagnostic that refused it |
| `skip-claim` | `path`, `line`, `reason`, `matched` | per `rust-mutants: skip` marker: where it sits, the reason its author wrote, and whether it hid anything |
| `kept` | `path`, `run_id` | per directory a run was asked to keep rather than remove, with the run that kept it |
| `route` | `mutant`, `index`, `granularity`, `fallback`, `reaching`, `considered`, `discharged`, `executed`, `reused` | per judged mutant: granularity, what widened it, the targets that could notice, the ones a proof discharged, the ones that ran, and the run an answer was reused from |
| `cache` | `mutant`, `key`, `hit`, `source_run_id` | what an earlier run of this exact tree said about one mutant: the key, whether a record answered, and the run that established it |
| `select` | `mutant`, `reason` | why one mutant was never executed |
| `identical` | `index`, `identity`, `detail` | what the equivalence layer said about one mutation: `identical`, `differs`, or nothing at all |
| `evidence` | `file`, `bytes`, `digest` | one file the run kept for an audit: its path, its size, and its digest |
| `mutant-exec` | `id`, `index`, `target`, `outcome`, `step_notice`, `exit_code`, `duration_ms`, `tests_run`, `signal`, `failed_tests`, `timeout_ms`, `timeout_source`, `alone` | the mutation, target, outcome, any verified step-limit notice, status, duration, tests run, signal, failed tests, the budget it was given and where that came from, and whether it had the machine to itself |
| `note` | `kind`, `detail` | a free-form note: progress, a decision, or a limitation |
| `run-end` | `outcome`, `error`, `events_emitted`, `events_dropped` | the outcome, any failure, and the recording's kept and lost event counts |

## Reading one back

`rust-mutants trace summary` reads the newest recording under the report directory and counts it: how many events of each type, what every phase took by its path through the nesting, how long each program was in, the slowest commands, how the executions and the routes came out, and how many rounds and bisections it cost.
`--run ID` reads a named one and `--dir DIR` reads one anywhere.

`rust-mutants trace check` says whether a recording is complete: it begins with `run-start`, ends with `run-end`, skipped no sequence number, dropped nothing, and closed every phase it opened.
It exits 1 when it is not, which is what a pipeline asks before it believes a recording.

`rust-mutants trace diff A B` names every count that is not the same in both,
which is what to read when a run got slower or a proof layer stopped removing work.

`rust-mutants explain <id>` renders what the catalog knows about one mutant,
and `rust-mutants why-skipped` tallies the skip reasons of a tree.
