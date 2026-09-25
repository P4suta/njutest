<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0034 — A binary is single-threaded only where nothing says otherwise

## Status

Accepted, 2026-09-24.
Implemented by `concurrency::scan`, `concurrency::proof` and `concurrency::explore`, by the runtime's delayed guard, by the `concurrency` record of every report part, the `schedule-dependent` finding, and the `schedule-not-explored` and `schedule-sampled` limitations.
The first slice of dimension C, `concurrent-v1`: the proof layer, before any schedule is explored.

## Context

A test binary that runs more than one thread passes on the schedule it happened to get.
Two runs of it can interleave differently, and a verdict read off one of them is a verdict about that interleaving.
Exploring schedules costs executions, and most binaries run one thread per test, where there is nothing to explore.
So the cheap half comes first: prove which binaries need nothing, and name every other one as a hole.

The run already has half the evidence.
The guards record the name of the thread that reached them, and libtest names each test's thread after the test ([ADR 0014](0014-the-guards-are-the-measurement.md)), so reach on a thread no test answers for is recorded as loose.
Loose reach shows a thread that touched a guard, but a thread that touches none leaves nothing, and a thread named exactly like a test is read as that test.
Reach alone cannot prove a binary single-threaded.

## Decision

1. **Single-threaded is a conjunction, and everything else is a hole.** A test binary is `single-threaded` only when its baseline reached no code off its tests' threads, its harness is libtest run with `--test-threads=1`, no package in its dependency closure has a place that can start a thread, and none of them links native code.
   Libtest runs a binary's tests on every processor unless it is told otherwise, and two tests on two threads interleave over whatever they share, so a binary run without `--test-threads=1` is `concurrent` with `parallel-tests`, however quiet its closure.
   The runner and the audit each read the harness arguments as libtest does, and both are held by their tests to one contract, `schema/libtest-harness-options.json`, which lists every option and whether it takes a value, so the two readings cannot drift apart.
   Where anything says it can run more than one thread it is `concurrent`, with every reason; where anything could not be looked at it is `not-proven`, with every reason.
   Both are holes until a schedule of them is explored: `schedule-not-explored` names them where none was.

2. **The source half reads tokens of the whole closure and fails closed.** Every `.rs` file of every package the binary links is read, registry packages included, as tokens, so a macro body and an attribute are read exactly as code is.
   A call or method whose name starts with `spawn` on any receiver, a path call to `scope`, rayon, crossbeam, a thread pool or a parallel iterator, a runtime's `main` or `test` attribute and `new_multi_thread` each make it `concurrent`.
   An `extern` block, a `links` key, and `pthread_create` make it `not-proven` as `native-code`, because code no Rust source here shows can start threads without a token saying so.
   A file that is not Rust's tokens, or not UTF-8, is `unread`, and never read as a file that starts nothing.
   What the closure compiled is what cargo reported of the session's own pristine build, and nothing inferred from a directory: every unit's inputs from the dep-info of exactly the units its `compiler-artifact` messages name, read by the engine's one dep-info reader, and every build script's linking from its `build-script-executed` message, whose `linked_libs` already hold every spelling of a library; a `rustc-link-arg` reaches no message, so the output beside the directory cargo gave that script is read for it, and an output that is not there is taken to link.
   A file the compiler read that is not `.rs` is code where a source of its package holds the tokens `include` `!`, however spaced or delimited, and data otherwise.
   A raw identifier is read as the name it spells, so `r#spawn` is `spawn`.
   A file is held to being Rust's tokens and parsed no further: no rule reads more than tokens, and a parser recurses through a chain of unary operators or nested generics that no limit on the source could bound.
   A file whose brackets nest deeper than 128 outside its comments and literals is `unread` before it is lexed, because its token tree is built and dropped recursively and would end the run on a small enough stack.
   A false "can start one" leaves a hole the report states; a false "cannot" would be a proof of nothing, so every uncertain token is read the first way.

3. **What is known to start a thread is said first.** A binary with a known spawn is `concurrent` even where something else of it could not be read, so the reason a reader acts on is not hidden behind the one they cannot.

4. **A doctest binary is never proven.** Its code is a doc string the token scan does not read, and rustdoc runs it where no reach is recorded; its standing is `not-proven` with `doctest`, named for what it is rather than for the reach it happens not to record.

5. **Every witnessed reason is re-derived by the audit.** The engine recording holds each binary's kind and harness, the harness arguments its baseline ran with, and its reach off its tests' threads.
   `proofaudit` derives `loose-reach`, `parallel-tests`, `no-touch`, `not-libtest` and `doctest` from them and requires the report to name exactly those, in the state they come to; the reasons a scan gives are stated as not re-derived.
   Shards of one run must agree on every record, and a part that measured a binary and records nothing about its threads is refused.

5. **A schedule is one guard delayed.** Asked for with `[schedules] explore = N`, a run takes up to N guards the baseline of a binary not proven single-threaded reached, chosen by the SHA-256 of their index so they spread across the catalog the same way every run, and for each starts one control in which every thread pauses 100 ms the first time it reaches that guard (`RUST_MUTANTS_DELAY`, which only a control carries: the engine and the runtime both refuse it beside an active mutant).
   The pause is bounded, one per thread per run, and the schedule is named by the guard a reader can go to, rather than by a seed nobody can read.

6. **A broken schedule is believed only when it repeats and the clean control passes.** A delayed control that failed is run twice more with the same guard delayed, and once with nothing delayed; only when both repeats fail and the undelayed control passes is the binary `broke` at that guard, a `schedule-dependent` finding and a defect.
   A control that ran past its bound under the pause, a repeat that passed, or an undelayed control that failed too settles nothing, and the guard is `undecided`.
   A binary every delayed guard passed is `sampled`, named by `schedule-sampled`: a sample of its schedules is never all of them.
   Only a whole run explores, since a shard's binaries are every part's.
   The proofaudit concurrency layer holds a `broke` to three failing delayed controls the engine recorded at that guard, and a `sampled` to a delayed control for every guard it names.

## Consequences

Most real projects state `schedule-not-explored` for their doctest binaries, and for every binary whose closure holds an async runtime or a parallel iterator.
That is the dimension saying where it cannot speak, which is what `whole-v1` will read as a hole.
Reading a large closure costs time once per package per run; the packages are immutable by id, so a cache keyed by id and version is the next change.

## Alternatives

A type-resolved analysis (rust-analyzer, or rustc's MIR) would name fewer false spawns, but it would put a second compiler front end into every run, and every false spawn it removes is a hole the report already states rather than a wrong proof.
