<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Engine trace

**Status: designed, not implemented.** Arrives with M1 step 3 and grows with
every step after it.

`Workspace::open` takes a trace sink through `OpenOptions`; the command line
writes JSON Lines under `--trace[=DIR]`. The rules are those of
[ADR 0002](../adr/0002-trace-is-not-evidence.md): never a claim, never a
failure, honest about drops.

| Type | Records |
| --- | --- |
| `open` | root, snapshot path and whether its stable name was available, sweep result |
| `snapshot` | files copied, bytes, digest, refusals |
| `exec` | every process: argv verbatim, dir, environment variable names, timeout, exit code, duration, output digest |
| `discover-file` | per file: candidates found, and every site with its form (`C`, `E`, `S`) or its skip reason |
| `validate-round` | per round: files instrumented, diagnostics read, which mutant each error was attributed to, files left unattributed |
| `bisect` | per bisect step: the set tried and the verdict |
| `instrument` | per file: guards placed, runtime identifier chosen, lines preserved |
| `build` | the test-binary build and the targets it produced |
| `mutant-exec` | id, target, args verbatim, outcome, duration, tests run |
| `probe-exec` | target, outcome, infected count |
| `witness` | per candidate: the witness placed and whether it checked |
| `run-end` | `events_emitted`, `events_dropped` |

`rust-mutants explain <id>` renders what the trace and the catalog know about
one mutant; `rust-mutants why-skipped` tallies the skip reasons of a tree;
`rust-mutants validate --explain` prints the attribution table of a
validation; `rust-mutants instrument --file F --print` writes the
instrumented source of one file to stdout.
