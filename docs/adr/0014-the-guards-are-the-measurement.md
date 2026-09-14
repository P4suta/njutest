<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0014 — The guards are the measurement

## Status

Accepted, 2026-09-07. Implemented by the `touch` module of rust-mutants and
the recording added to the runtime of [ADR 0011](0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)
(E10).

## Context

Routing rested on an LLVM coverage build: compile the whole graph with
`-C instrument-coverage`, run every target once, and read the regions each
target executed. Two things follow from that, and both are expensive.

The flags reach the build through `RUSTFLAGS`, which changes the fingerprint
of every crate in the dependency graph, so the measurement is a second full
build of everything — not of the workspace, of the graph. On a workspace with
sixty test targets that is the single largest thing a run does, and stable
Rust has no way to ask for it on the workspace's own crates alone
(`-Z profile-rustflags` is nightly).

And a region places a mutation in a **target**. A target placed there then
runs every test it has for that one mutation. A library with two hundred tests
runs two hundred of them to find out what one of them would have said.

Meanwhile the engine already runs every target once with nothing activated, to
refuse a session whose instrumented baseline does not pass. That run executes
every guard the instrumented tree carries.

## Decision

The guards report what they reached, on that run.

`RUST_MUTANTS_TOUCH` names a log and `RUST_MUTANTS_CATALOG` says which catalog
it is about; a process records only when the catalog it was built from is that
one. When both agree, `active(index)` records the
index against the thread that reached it before it answers; libtest gives each
test a thread of its own named after the test, on every platform that has
threads and at every concurrency level (`library/test/src/lib.rs`,
`thread::Builder::new().name(desc.name)`, reached from the one-thread path as
well). So the record is **per test**, and it costs the run nothing it was not
already spending.

Each thread buffers what it reached and appends one line on its way out. A
thread nothing can name a test after — the main one, a benchmark, one a test
spawned for itself — writes `-`, and everything under that name reaches
**every** test of its target. A process that cannot record exits 96 rather
than running on: silence is exactly what licenses a run to skip a test.

Routing then narrows twice. A target no test of which reached the mutation is
not run at all. A target some of whose tests reached it runs exactly those
tests, in one process, because libtest takes every free argument as a filter
and `--exact` applies to all of them.

A filtered process only answers about the mutation if the same filter passes
without it, so the first time a set of tests is named it is put once with
nothing active and the answer is remembered. The mutants of one function are
covered by one set, so that is one process for all of them; a set that does
not pass takes its target off test routing and runs every test of it, and the
run says `test-routing-unsound` about it.

The coverage build stays, behind `--coverage`, as an independent second
opinion the differential harness holds the guards to.

## Consequences

- The default run makes no coverage build and needs no `llvm-profdata`, and
  that includes `branch-never-taken`: the marker at a body's first statement
  is written into the witness tree as well, so the one `cargo check` that
  sifts the type witnesses sifts the markers too and no build is added.
- A mutation is put to the tests that reached it. On `fixture-coverage` the
  guards start 18 tests where the regions start 42 and a run with nothing
  removed starts 46; the process counts are 15 and 14, which is why the work
  ledger counts both.
- On this repository's own engine — 5,680 mutants against 67 test targets —
  preparing takes 490 s (one pristine build, one `cargo check` for the
  witnesses and the markers, one instrumented build with its validation
  rounds, and the one run of every target that is both the baseline and the
  measurement). It removes 87.6% of the processes and **93.6% of the tests**:
  197,949 of 3,106,960. 4,802 of the 5,680 routes are at test granularity and
  878 at target granularity. Of the record itself, 182,816 (test, site) pairs
  were attributed and 756 were touches nothing could be attributed to.
- The guards are more precise than the regions, not only cheaper. Every
  fixture fate that moved when this landed moved from `survived` to
  `unreached`: `fixture-families` never calls `conditions`, and the coverage
  build placed mutations inside it anyway. No kill was lost.
- Every file carries the recording, so it is paid for once per instrumented
  file at compile time: about 9 ms and 1.5 MB each, measured in ADR 0011.
- Where a platform runs tests without a thread each — wasm, emscripten, zkvm —
  every touch is unattributable and every mutation goes to every test of its
  target. `rust-mutants doctor` says so before a run rather than after.
- What was measured is kept as `touched-v1.json`, and `cargo xtask
  engine-audit` re-decides every route from it without the engine. Four ways
  of being wrong are tried against it in the suite and each is a violation.

## Alternatives

- **Evaluate several mutants in one process, one per test thread.** libtest
  gives each test a thread, so it is technically possible. It rests on
  process-wide state — a `static`, a `OnceLock`, a temporary directory — not
  leaking between mutants, which is an assumption about the project under test
  that nothing can check. [ADR 0004](0004-proof-layers-not-budgets.md) decision
  1 rules out unverifiable assumptions. `fork` is no better: libtest holds
  threads.
- **Instrument only the workspace's own crates for coverage.** There is no
  stable way to ask for it.
- **Infer that a body ran from a mutant inside it having been touched.** A
  mutant inside a body is not necessarily executed when the body is entered —
  it may sit in a nested branch — so the contrapositive the proof needs does
  not hold. The marker the instrumenter writes at the body's *first statement*
  does, which is why `branch-never-taken` rests on one rather than on that.
- **A budget, a sample, or a list of slow targets to skip.** ADR 0004
  decision 1.
