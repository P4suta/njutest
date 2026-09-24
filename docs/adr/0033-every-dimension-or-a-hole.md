<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0033 — Every dimension, or a hole

## Status

Proposed, 2026-09-24.
The contract `whole-v1` and the assurance matrix of the plan's Part III.

## Context

A run now measures along more than one dimension: what a mutation changes ([ADR 0004](0004-proof-layers-not-budgets.md)), what a seam is asked ([ADR 0021](0021-a-claim-is-a-perturbation-an-observer-and-a-decision.md)), what a knob sets differently ([ADR 0031](0031-a-knob-is-one-control-started-differently.md)), and what a failed call does ([ADR 0032](0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
Schedules and durability are planned.
Each has its own closed set of decisions, and the report reads them part by part.

Two things are missing.
No page says, dimension by dimension, what a run measured, what it left open, and what it cannot speak about at all, so a reader deciding whether to ship reads six sections and adds them up.
And every contract lets a dimension nobody asked about pass in silence: a run that measured mutations and nothing else is `ASSURED` about a suite that has never been asked what happens when a call fails.
The tree's own rule — what was not measured is not claimed as measured — stops at the dimension.

## Decision

1. **A dimension is a closed set, and every one of them is a column.** `Dimension` is `mutation`, `repeatable`, `fault`, `schedule`, `wire`, `durable`, derived `AllVariants`.
   Each column comes to one closed `Column`:
   - `measured`, with `catalogued` (what the dimension could have asked), `answered` (what it decided), `holes` (what it put and could not decide), and `speaks_not_about` (the classes it cannot put at all, each named, never counted into a hole);
   - `unmeasured`, with why: the run was asked and could not measure the dimension at all, as when the tree with every fault guarded gives no baseline;
   - `not-asked`: the run could have measured it and did not;
   - `nothing-to-ask`, with why: the run looked and there was nothing to put, as in a tree with no `?` in a measured file;
   - `not-in-this-release`: no measurement of the dimension exists yet.
   Every record is placed as answered, a hole, or a class not spoken about by one exhaustive match over its decision, so a decision added later is one somebody places; `catalogued = answered + holes` holds by construction, and a count that would not fit is `unmeasured`.
   Where a report holds several builds, a column is every build's counts added where each measured the dimension, and otherwise the column of the build that established least: a hole in any build is a hole of all of them.

2. **The matrix is derived, never stored.** Each column is computed from the records the part already holds: the mutation accounting, the knob records, the fault records, the seam records.
   A stored column would be a second copy of those records that a reader could find disagreeing with them.
   The record stream carries one `DIMENSION` record per column, and the drawing ends with the matrix.

3. **`whole-v1` asks every dimension, and a hole in any of them is not assured.** `whole-v1` runs the soundness phase as `deep-v1` does, puts every fault, and sets every knob.
   It concludes `INSUFFICIENT` whenever a column is `not-asked` or `not-in-this-release`, or is `measured` with a hole; `nothing-to-ask` and `speaks_not_about` are stated and are not holes.
   Each such column is a `not-measured` finding naming the dimension, so the verdict is decided by findings as every other verdict is.
   Every other contract reads the matrix and is decided exactly as it was.

4. **Contracts are answered by what they ask, never by comparing names.** The places that asked `contract == verified-v1` or `contract != deep-v1` now ask the contract what it runs — `runs_miri`, `proves_models`, `asks_every_dimension` — each an exhaustive match, so a contract added later is one the compiler makes somebody place.

5. **The default does not change yet.** The plan makes `whole-v1` the default for a run that names no contract.
   Until schedules and durability are measured, every `whole-v1` run is `INSUFFICIENT` by construction, and a default that nothing can satisfy is not a strict answer but a useless one.
   The default moves to `whole-v1` in the change that ships the last `not-in-this-release` column; `standard-v1`, `deep-v1` and `verified-v1` stay what they are today.

## Consequences

- A reader deciding whether to ship reads one table, and a dimension nobody asked about is a row saying so rather than an absence.
- `whole-v1` costs everything every dimension costs: a second instrumented build for faults, one control per knob per target, Miri over every crate with unsafe code.
- A dimension added later is a variant of `Dimension`, which the compiler makes the column, the record stream and the drawing name before anything builds.
