<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Engine trace

**Status: implemented.** `rust_mutants::trace` holds the `Recorder`, the
`Sink` enum with `MemorySink`, `DirSink`, and `Tee`, `read_events` and
`check` for reading a stream back, and `summary` for reading one as numbers.
The command line records under `--trace[=DIR]` and reads a recording back
with `rust-mutants trace summary|check|diff`.

The rules are those of
[ADR 0002](../adr/0002-trace-is-not-evidence.md): a recording is never a
claim, never a failure, and honest about what it dropped. A directory that
cannot be created costs one line on standard error and nothing else; the
command does what it was asked either way.

## Where a recording goes

`--trace` names a directory, and without one the engine picks the place a
reader would look. A run records beside its own report, under
`<reports.directory>/<run id>/trace/`; every other command records under
`<reports.directory>/traces/<run id>-<command>/`, which is outside what a
snapshot copies. The run is named before the workspace is opened, so the
opening itself is recorded. `--trace --no-report` still makes the run's
directory. Both places are pruned by `reports.keep`, counted separately, so a
recording never costs a stored report its place.

A recording owns its directory: a directory that is already there is refused
rather than appended to, which is what keeps two runs from writing one
stream.

## What a line is

Every line is one JSON object: `seq` (monotonic from 1, delivery order is
sequence order however many threads record), `timestamp` (RFC 3339, UTC),
`elapsed_ms`, `type`, and the type's record under a key named after the
type. The first event is `run-start` with `schema: "rust-mutants-trace-v1"`
and the engine version; the last is `run-end` with `outcome`,
`events_emitted`, and `events_dropped`, where a sink that counts its own
drops (a full ring, a failed write) is the authority and the recorder's
observed failures fill in otherwise. The outcome is the word the
command's exit code is named after: `detected` when everything the run
executed was noticed and there is nothing to report, `undetected` when
something was not, `failed` when the command itself could not finish, and
`interrupted` when it was stopped.

An `exec` record carries environment variable *names* only (the recorder
strips `=value`), and the output as `output_bytes` plus `output_sha256`; a
`DirSink` preserves the capture beside the stream under `output/<seq>.txt`,
cut at 1 MiB with a `...` marker, and records `output_path` and
`output_truncated`. The disabled trace is `Recorder::disabled()`; call sites
record unconditionally. `Recorder::new` takes the clock as an argument, which
is how the goldens in `crates/rust-mutants/tests/testdata/trace/` freeze the
shape, and every line is checked against
[`schema/rust-mutants-trace-v1.json`](../../schema/rust-mutants-trace-v1.json).

## The types

| Type | Records |
| --- | --- |
| `run-start` | schema, engine version |
| `phase-start` / `phase-end` | a phase boundary: name, and on the end its duration |
| `open` | root, snapshot path and whether its stable name was available, sweep result |
| `snapshot` | files copied, bytes, digest, refusals |
| `exec` | every process: argv verbatim, dir, environment variable names, timeout, exit code, duration, output digest |
| `discover-file` | per file: candidates found, and every site with its form (`C`, `E`, `S`) or its skip reason |
| `instrument` | per file: guards placed, the runtime module's name, and the line count before and after, which must be equal |
| `validate-round` | per round: how many were condemned going in, how many files it had to write again, whether the tree compiled, which mutant each error was attributed to with the compiler's first line, and the errors no branch accounts for |
| `bisect` | per isolation: how many suspects, which offenders it named, how many compilations it cost, how many it could put the compiler's own words to |
| `build` | the test binaries the build produced, each with its kind, whether it carries the libtest harness, and what a run could not establish about it |
| `verify` | per target: what the suite established with nothing active, how many tests ran, how long the baseline took, and whether that exact passing measurement was remembered |
| `touch` | per target: how many of its tests reached a mutation, how many distinct mutations anything of it reached, how many were reached where nothing named a test, and how many it saw a guard's two branches differ over |
| `witness` | per candidate: the witness placed, whether it checked, and the diagnostic that refused it |
| `skip-claim` | per `rust-mutants: skip` marker: where it sits, the reason its author wrote, and whether it hid anything |
| `kept` | per directory a run was asked to keep rather than remove, with the run that kept it |
| `route` | per judged mutant: granularity, what widened it, the targets that could notice, the ones a proof discharged, the ones that ran, and the run an answer was reused from |
| `mutant-exec` | id, index, target, outcome, exit code, duration, tests run, signal, failed tests, the budget it was given and where that came from, and whether it had the machine to itself |
| `cache` | what an earlier run of this exact tree said about one mutant: the key, whether a record answered, and the run that established it |
| `select` | why one mutant was never executed |
| `identical` | what the equivalence layer said about one mutation: `identical`, `differs`, or nothing at all |
| `evidence` | one file the run kept for an audit: its path, its size, and its digest |
| `note` | a free-form note: progress, a decision, a limitation |
| `run-end` | `outcome`, `events_emitted`, `events_dropped` |

## Reading one back

`rust-mutants trace summary` reads the newest recording under the report
directory and counts it: how many events of each type, what every phase took
by its path through the nesting, how long each program was in, the slowest
commands, how the executions and the routes came out, and how many rounds and
bisections it cost. `--run ID` reads a named one and `--dir DIR` reads one
anywhere.

`rust-mutants trace check` says whether a recording is complete: it begins
with `run-start`, ends with `run-end`, skipped no sequence number, dropped
nothing, and closed every phase it opened. It exits 1 when it is not, which
is what a pipeline asks before it believes a recording.

`rust-mutants trace diff A B` names every count that is not the same in both,
which is what to read when a run got slower or a proof layer stopped
removing work.

`rust-mutants explain <id>` renders what the catalog knows about one mutant,
and `rust-mutants why-skipped` tallies the skip reasons of a tree.
