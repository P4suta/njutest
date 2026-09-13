<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0003 — No general record/replay engine

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0003. The scripted fakes it
prefers are the `testkit` modules; the recording it declines to consume is
[trace v1](../trace-v1.md).

*This record is about record/replay of process execution as a test
technique. It is unrelated to `njutest replay ID`, which re-runs a recorded
finding through the real toolchain.*

## Context

A trace already records the record half of record/replay: for every command,
the argument vector, working directory, environment variable names, timeout,
exit code, duration, and the captured output preserved beside the stream. The
obvious next step is to feed a recording back into the runner so a failure
reproduces without a toolchain.

The suite does not need an engine to get that. Everything the runner
executes passes through two narrow traits — `CommandWorkspace` and
`MutationSession` — and the testkit answers both from prefix rule tables that
fail closed on a command no rule covers and record what was attempted. A test
states its scenario in a few lines, and the scenario is legible in the test.
An engine would be a standing obligation: matching policy, versioning, and a
second execution path through the runner that must stay honest as the first
changes.

## Decision

No general record/replay engine, and no replay execution path. Recording is
what the trace does; standing in for an execution is what the hand-written
scripted fakes do. Traces are read by people.

The event schema keeps the fields a future bridge would need lossless: argv
and args verbatim, the working directory, the exit code, the timed-out flag,
the duration, and the captured output as a file. Any change that compresses
or normalizes those fields revisits this record.

**Revisit when** transcribing a trace into scripted rules by hand becomes a
recurring cost. The answer then is the smallest thing: a testkit helper that
reads a `trace.jsonl` and registers one rule per `exec` event.

## Consequences

- Test doubles stay explicit and legible.
- Traces stay a debugging artifact rather than a test input; no golden
  recording ages.
- Reproducing a complicated failure in a test means reading a trace and
  writing rules by hand.
