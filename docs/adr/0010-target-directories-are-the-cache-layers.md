<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0010 — Target directories are the cache layers

## Status

Superseded, 2026-09-11, by
[ADR 0019](0019-the-engine-owns-the-compiled-build-cache.md).

## Context

[ADR 0005](0005-build-cache-njutest-owns.md) needs two layers and a rule
about which commands write to which. Go offers `GOCACHEPROG`, a protocol by
which the toolchain asks a program for every cache entry. Cargo has no such
protocol; `target/` is the cache, and `CARGO_TARGET_DIR` and `--target-dir`
say where it is.

## Decision

- The base layer is a set of target directories under the machine's build
  root: `native/<rustc commit>/` for `cargo test --no-run` and the doctest
  build, `coverage/<rustc commit>/` for the `-C instrument-coverage` build
  (RUSTFLAGS invalidate every fingerprint, so the two never share), and
  `mutants/<rustc commit>/` handed to rust-mutants.
- Commands that compile receive `--target-dir <base layer>` on their command
  line. **Every process that runs tests** — a baseline target, an original
  control, the engine's executions, a provider — receives
  `CARGO_TARGET_DIR=<run scratch>/build` in its environment, so any `cargo`
  a test suite spawns writes to the scratch layer and dies with the run.
  `--target-dir` outranks the environment, which is what lets one command
  compile into the base layer while its children stay in scratch.
- The rule is `build_cache::layer_for(command)`, one function, and a test
  enumerates every argument-vector builder the runner has and pins its side.
- Collection is by file modification time, cargo-sweep style, under a
  non-blocking lock on the layer and cargo's own `.cargo-lock`; cargo rebuilds
  whatever a partial removal took.

## Consequences

- No cache-program subcommand, no protocol, no re-execution of the binary.
- Three flavours of a debug build cost disk; the default bound is 8 GiB and
  is per machine.
- A stable snapshot path per repository root (the engine's) plus a stable
  target directory (the runner's) is what makes successive runs incremental.
