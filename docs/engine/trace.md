<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Engine trace

**Status: recorder, sinks, and reader implemented (M1 step 3); the
vocabulary grows with every step after it.** `rust_mutants::trace` holds
the `Recorder`, the `Sink` trait with `MemorySink`, `WriterSink`, and
`DirSink`, and `read_events` / `check` for reading a stream back.

`Workspace::open` takes a trace sink through `OpenOptions`; the command line
writes JSON Lines under `--trace[=DIR]`. The rules are those of
[ADR 0002](../adr/0002-trace-is-not-evidence.md): never a claim, never a
failure, honest about drops.

Every line is one JSON object: `seq` (monotonic from 1, delivery order is
sequence order however many threads record), `timestamp` (RFC 3339, UTC),
`elapsed_ms`, `type`, and the type's record under a key named after the
type. The first event is `run-start` with `schema: "rust-mutants-trace-v1"`
and the engine version; the last is `run-end` with `events_emitted` and
`events_dropped`, where a sink that counts its own drops (a full ring, a
failed write) is the authority and the recorder's observed failures fill in
otherwise. An `exec` record carries environment variable *names* only (the
recorder strips `=value`), and the output as `output_bytes` plus
`output_sha256`; a `DirSink` preserves the capture beside the stream under
`output/<seq>.txt`, cut at 1 MiB with a `...` marker, and records
`output_path` and `output_truncated`. The disabled trace is
`Recorder::disabled()`; call sites record unconditionally. `Recorder::new`
takes the clock as an argument, which is how the golden in
`crates/rust-mutants/tests/testdata/trace/basic.golden` freezes the shape.

| Type | Records |
| --- | --- |
| `run-start` | schema, engine version |
| `phase-start` / `phase-end` | a phase boundary: name, and on the end its duration |
| `open` | root, snapshot path and whether its stable name was available, sweep result |
| `snapshot` | files copied, bytes, digest, refusals |
| `exec` | every process: argv verbatim, dir, environment variable names, timeout, exit code, duration, output digest |
| `note` | a free-form note: progress, a decision, a limitation |
| `instrument` | per file: guards placed, the runtime module's name, and the line count before and after, which must be equal |
| `validate-round` | per round: how many were condemned going in, whether the tree compiled, which mutant each error was attributed to with the compiler's first line, and the errors no branch accounts for |
| `bisect` | per isolation: how many suspects, which offenders it named, how many compilations it cost |
| `build` | the test binaries the build produced |
| `mutant-exec` | id, index, target, outcome, exit code, duration, tests run |
| `discover-file` | per file: candidates found, and every site with its form (`C`, `E`, `S`) or its skip reason |
| `probe-exec` | target, outcome, infected count |
| `witness` | per candidate: the witness placed and whether it checked |
| `run-end` | `events_emitted`, `events_dropped` |

`rust-mutants explain <id>` renders what the trace and the catalog know about
one mutant; `rust-mutants why-skipped` tallies the skip reasons of a tree;
`rust-mutants validate --explain` prints the attribution table of a
validation; `rust-mutants instrument --file F --print` writes the
instrumented source of one file to stdout.
