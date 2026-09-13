<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0002 — A trace is not evidence

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0002. Implemented by the
`trace` module of `njutest-cli` and the `trace` sink of `rust-mutants` as the
milestones deliver them; the rule applies from the first event.

## Context

njutest is fail-closed about assurance: an input it cannot identify, a
metadata field it cannot reach, or an output it cannot digest ends the run
with an error rather than an optimistic verdict. Execution tracing has the
opposite failure profile. A trace is diagnostic exhaust a developer reads to
learn why a run behaved as it did, often on a machine they cannot attach a
debugger to. Applying fail-closed to that output would make the tool less
reliable than it was without it: a full disk or a read-only directory would
end a verification that was otherwise sound. But "best effort" is where
diagnostics start lying — a trace that silently dropped a third of its events
sends a reader hunting for the absence of a command that ran.

## Decision

A trace is diagnostic exhaust and is never evidence.

1. **It takes no part in any claim.** No trace option enters the mode
   identity, the assurance inputs, or the evidence digest; a traced and an
   untraced run of the same repository share a cache identity and reach the
   same verdict.
2. **It never costs the run.** Sink failures are counted, never returned. A
   directory that cannot be created costs one `trace-unavailable` note; the
   run continues untraced.
3. **A directory the snapshot would read is refused as a trace**, except
   `.njutest/`, which the snapshot never reads.
4. **Honesty replaces fail-closed.** Every sink counts its drops, every
   recording ends with `events_emitted` and `events_dropped`, every line is
   flushed as written. A reader can tell a complete recording from a lossy
   one and a killed run from a finished one.
5. **A trace is secret safe.** An exec event records environment variable
   names alone, and the recorder — not its callers — reduces an entry to its
   name. Captured output is digested into the event and preserved beside it.
6. **The engine records the same way.** rust-mutants takes a trace sink
   through `OpenOptions` and records open, snapshot, discovery (every site's
   form and skip reason), validation (every round, attribution, and bisect
   step), instrumentation, builds, executions, and probes, under the same
   rules: never a claim, never a failure, always honest about drops.

The disabled trace is `Recorder::disabled()`; call sites record
unconditionally, which keeps the traced and untraced paths identical.

## Consequences

- A trace can never be cited as proof. "Did this run really execute that
  target" is answered by the report; the trace says what the run appeared to
  do while it did it.
- A reader checks two things before trusting a recording: the last line is
  `run-end` and `events_dropped` is zero.
- Tracing a warm run records the cache hit, not the work.
- `proofaudit` reads traces because a trace is the only recording of routing
  decisions; that it audits from exhaust is exactly why it re-derives every
  rule instead of trusting the recorder.
