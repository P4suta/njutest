<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0005 — A build cache mjutest owns, and what may write to it

## Status

Superseded, 2026-09-11, by
[ADR 0019](0019-the-engine-owns-the-compiled-build-cache.md).

## Context

A verification leaves gigabytes behind. goatest measured about 5.5 GB per
hour of build cache growth; a single run added 14 GB, and three agents
sharing one disk filled it. The garbage is not mutants — an engine compiles a
tree per run, not per mutant. It is the builds the *test suites themselves*
perform: a suite that compiles fixture projects, runs a golden build, or
spawns `cargo` writes into a cache addressed by a path nothing will ever ask
for again. Two things follow: the disk fills with entries that can never be
hit, and the work that *would* be hit again — the standard library, the
dependencies, the project's own crates — is evicted by that garbage.

## Decision

1. **mjutest owns its build cache, in two layers.** A *base* layer belongs to
   the machine and survives between runs; a *scratch* layer belongs to one
   run and is removed when it ends.
2. **Only a command that compiles or lists may write to the base layer**:
   `cargo metadata`, `cargo test --no-run`, `cargo build`, `cargo check`, and
   the doctest build.
3. **Nothing that runs the project's tests may.** Baseline target runs,
   original controls, the engine's sessions, and candidate validation write to
   the run's scratch layer, which dies with the run. This is the load-bearing
   half: a suite's own `cargo` invocations are exactly what produce the
   throwaway builds, and were they to persist they would evict what the base
   layer exists to hold.
4. **The rule lives in one function and is pinned by a test** that names
   every command mjutest issues and which side of the rule it falls on.
5. **Every run bounds the base layer.** `[cache] build_max_bytes` bounds it —
   8 GiB by default, because a debug build of a Rust workspace in three
   flavours is measured in gigabytes — and `[cache] build_dir` says where it
   lives, per machine by default. A run collects the layer when it ends under
   a non-blocking lock; `mjutest cache gc` runs the same collection on demand.
6. **A layer is a directory mjutest made, and the marker proves it.** A
   directory that exists, holds files, and carries none of mjutest's names is
   refused without writing anything into it. The marker is
   `mjutest-build-cache-v1`.
7. **The cache is never a reason to fail.** A layer that cannot be created is
   a progress note and done without.
8. **Only the composition root names the cache root.** `cache status` and
   `cache gc` inspect and collect that directory, so a service embedded
   elsewhere or a test binary that resolved it for itself would be deleting
   entries out of the developer's own build cache.

## Consequences

- The layout is versioned in the directory name (`build-v1`) and the marker.
  A later layout is a new directory, not a migration.
- A build of identical source at a different absolute path misses for the
  project's own crates, because cargo hashes package paths into fingerprints.
  mjutest does not remap paths — that would change the binaries under
  verification — and instead gives rust-mutants a snapshot path that is
  stable per repository root, so successive runs hit each other.
- What a run asked the cache for is reported as a progress note, so a reader
  who sees mjutest go faster can see how much of it was the cache — the same
  rule [ADR 0004](0004-proof-layers-not-budgets.md) asks of a proof layer.
