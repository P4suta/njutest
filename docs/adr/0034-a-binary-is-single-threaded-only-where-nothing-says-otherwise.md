<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0034 — A binary is single-threaded only where nothing says otherwise

## Status

Accepted, 2026-09-24.
Implemented by `concurrency::scan` and `concurrency::proof`, by the `concurrency` record of every report part and the `schedule-not-explored` limitation.
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

1. **Single-threaded is a conjunction, and everything else is a hole.** A test binary is `single-threaded` only when its baseline reached no code off its tests' threads, its harness is libtest, no package in its dependency closure has a place that can start a thread, and none of them links native code.
   Where anything says it can run more than one thread it is `concurrent`, with every reason; where anything could not be looked at it is `not-proven`, with every reason.
   Both are named by `schedule-not-explored`, since nothing explores a schedule yet.

2. **The source half reads tokens of the whole closure and fails closed.** Every `.rs` file of every package the binary links is read, registry packages included, as tokens, so a macro body and an attribute are read exactly as code is.
   A call or method whose name starts with `spawn` on any receiver, a path call to `scope`, rayon, crossbeam, a thread pool or a parallel iterator, a runtime's `main` or `test` attribute and `new_multi_thread` each make it `concurrent`.
   An `extern` block, a `links` key, and `pthread_create` make it `not-proven` as `native-code`, because code no Rust source here shows can start threads without a token saying so.
   A file that is not Rust, or not UTF-8, is `unread`, and never read as a file that starts nothing.
   A false "can start one" leaves a hole the report states; a false "cannot" would be a proof of nothing, so every uncertain token is read the first way.

3. **What is known to start a thread is said first.** A binary with a known spawn is `concurrent` even where something else of it could not be read, so the reason a reader acts on is not hidden behind the one they cannot.

4. **A doctest binary is never proven.** Its code is a doc string the token scan does not read, and rustdoc runs it where no reach is recorded.

## Consequences

Most real projects state `schedule-not-explored` for their doctest binaries, and for every binary whose closure holds an async runtime or a parallel iterator.
That is the dimension saying where it cannot speak, which is what `whole-v1` will read as a hole.
Reading a large closure costs time once per package per run; the packages are immutable by id, so a cache keyed by id and version is the next change.

## Alternatives

A type-resolved analysis (rust-analyzer, or rustc's MIR) would name fewer false spawns, but it would put a second compiler front end into every run, and every false spawn it removes is a hole the report already states rather than a wrong proof.
