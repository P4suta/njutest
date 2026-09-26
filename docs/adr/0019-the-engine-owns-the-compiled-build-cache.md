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

## Amendment, 2026-09-26: a unit is fresh only for the bytes it was built from

### Context

Cargo decides whether a unit is fresh by comparing the modification times of the files it read with the unit's own.
The copy a run builds keeps the time each file was written, on purpose, so that a second run compiles only what changed.
The engine rewrites files the time does not describe: an instrumented file is the same file with other bytes.
A member one run instrumented and a later run leaves as written therefore carries a time older than the instrumented unit in the shared directory, and cargo links that unit.

Measured on storage-scout: a run cataloging only `crates/cli/src/watch.rs` linked a `storage-scout-core` an earlier run had instrumented with another catalog, built by an older release.
Every test that entered it stopped at the old runtime's step check, which exited 94 and said nothing; nine mutants were errored, and a baseline could fail for reasons no source held.
The fixture reproduction is two runs of `fixture-witness-downstream` sharing a target directory: the whole catalog, then `--include crates/downstream/src/lib.rs`, whose two mutants errored with exit 94.

### Decision

1. Every target directory a build writes into keeps `rust-mutants-built-v1.json`: for each workspace member, the digest of every file of the copy under its directory as the last build that could write its units found them, and the moment that digest was recorded.
2. Before cargo runs, `compile` settles the directory: a member whose digest differs from the record, or that the record does not name, loses every fingerprint cargo keeps for it, under every profile and target triple, and only then is the record rewritten with the new digest and the present moment.
   A unit without a fingerprint is one cargo compiles again, and a unit that depends on it follows.
3. Settling then gives every file of every member the moment its member's digest was recorded.
   That moment is older than every unit built from those bytes, since the member's older units were removed at it, and newer than every unit built from any others.
   So a file's time says what cargo needs to know whoever wrote it, and bytes the engine writes again the same, as instrumenting an unchanged tree does, compile nothing.
4. `CompileOptions` names its target directory as a `BuildDir`, which carries the members, so no build into a shared directory can skip the settling.
5. A directory inside another that keeps its own record, such as `witness`, `coverage` or `pristine`, is another target directory; settling the outer one passes over it.
   The copy as it was written is checked in `pristine`, apart from the instrumented builds, because one directory given the two trees in turn would compile each of them every run.
6. A record that cannot be read, or that is not one this release writes, is `RM1022`, rather than a record the run trusts or silently replaces.

### Consequences

- A unit cargo judges fresh was compiled from the bytes the copy holds now, whatever an earlier run, an earlier release, or another catalog wrote there.
- A directory an earlier release filled has no record, so the first run after an upgrade compiles every member again, once; dependencies are not members and keep their units.
- A member whose bytes are the same as last time keeps every unit, even where the engine wrote them again, so a repeat run of an unchanged tree compiles nothing: on `fixture-simple` the third run rewrote no fingerprint, where it used to rewrite every member unit's.
- The record is per member and not per file, so an edit to one file compiles its whole member again, which cargo would have done for the unit that file belongs to.
- A file no member's directory holds keeps the time it was written, which only a person changes, and cargo reads that time as it always has.
- The pristine check moved into `pristine`, so a path under the target directory that a unit's dep-info names now begins `$target/pristine/`, and outcome keys that name one change once.
- Documentation examples run through `cargo test --doc` against the tree the last settled build compiled; nothing writes the tree between that build and them.
