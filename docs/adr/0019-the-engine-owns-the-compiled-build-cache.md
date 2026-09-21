<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0019 — The engine owns the compiled build cache

## Status

Accepted, 2026-09-11.
Supersedes [ADR 0005](0005-build-cache-njutest-owns.md) and [ADR 0010](0010-target-directories-are-the-cache-layers.md).

## Context

The runner once owned a second machine-wide build-cache hierarchy, its configuration, marker, status, and collector.
The implemented verification path no longer compiled through those layers: rust-mutants prepares the instrumented snapshot and builds it into a target directory whose name is stable for the source root.
Keeping a second owner left configuration and garbage collection for artifacts no run produced.

Cargo target directories are compiled state.
Two programs collecting the same class of state cannot agree about liveness from their own locks and markers, and a status line from the program that did not build the artifacts does not identify their owner.

## Decision

1. rust-mutants' stable target directory is the only persistent compiled build cache used by a verification.
   Its lifecycle, locking, and collection belong to the engine.
2. njutest owns no persistent compiled layer.
   Its `[cache]` table controls only outcome answers: `max_bytes`, `ttl`, export, and import.
3. Cargo started from a test process remains isolated with `CARGO_TARGET_DIR` under that run's `Scratch`.
   This is disposable process isolation, not a cache shared across runs.
4. `njutest cache` neither reports nor collects compiled artifacts.
   The retired `build_dir` and `build_max_bytes` keys are rejected as unknown fields, and an upgrade removes them.

## Consequences

- There is one owner and one command surface for persistent compiled state.
- A runner run still removes builds started by the project under test when its scratch closes.
- Existing configuration carrying either retired key stops with a named parse error instead of silently keeping a setting that does nothing.
