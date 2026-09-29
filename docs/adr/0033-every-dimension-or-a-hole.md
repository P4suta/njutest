<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0033 — Every dimension, or a hole

## Status

Accepted, 2026-09-29.
Proposed 2026-09-24, and accepted once every decision below was held by the code, a test and an audit layer.
The contract `whole-v1` and the assurance matrix of the plan's Part III.
Decision 5 happened in the change that measured the schedule column, and `whole-v1` is the default.

| Decision | Held by |
| --- | --- |
| 1, a closed set of columns | `report::matrix`: `Dimension`, `Column`, and `Counted`, every record placed by one exhaustive match and every hole named with why; the mutation column's `skipped`, one match over `SkipReason`; a baseline every shard shares read once per build; a tree whose baseline did not build `unmeasured` along every dimension; `crates/njutest/tests/matrix.rs`, `report_merge::a_binary_every_shard_ran_is_one_hole_of_the_merge_however_many_shards_ran_it`; the proofaudit `dimensions` layer, which re-derives the holes of a flat part and of every part of a merge, and counts a lead as a hole as the runner does |
| 2, derived, never stored | `matrix::rows` over `Evidence::of` and the report's `MatrixEvidence`; the `DIMENSION` record with `open=` and `speaks_not_about=`, and the drawing beneath each line; `report.golden.lines`, `gallery.golden`, `matrix::the_drawing_says_what_a_dimension_does_not_speak_about`; the audit holds a kept stream to one `DIMENSION` record per dimension, each saying what the records say of its holes, with `catalogued = answered + holes`, in `proofaudit::a_kept_stream_whose_column_the_records_contradict_is_refused` |
| 3, `whole-v1` and its findings | `Config::asked_everything` and `asks_every_dimension`; `matrix::holes` raised once over the pooled matrix and derived again by `report::derived`; `matrix::a_whole_contract_puts_every_fault_and_knob_and_refuses_a_document_that_says_not_to`, `a_whole_contract_accepts_every_knob_named_in_any_order`, `the_schedule_row_names_which_binary_is_open_and_why`, `configured::a_dimension_several_builds_leave_open_is_one_finding_of_the_run`, `toolchain_matrix`; the proofaudit `dimensions` layer for a flat part's findings, and the merge layer's `dimensions` rule for a merge's, with its planted defect and `proofaudit::a_merged_stream_that_calls_a_dimension_established_where_the_parts_leave_it_open_is_refused` |
| 4, contracts by what they ask | `Contract::runs_miri`, `proves_models`, `asks_every_dimension` and `name`, each one exhaustive match; `Config::verified` and `Report::checked` ask `proves_models`; the evidence key holds a `Contract`; `evidence_key::a_key_spells_each_contract_as_a_document_names_it` |
| 5, the default | `Contract::PROTOCOL_DEFAULT`; `toolchain_matrix::a_run_that_names_no_contract_asks_every_dimension`; `report_merge::a_whole_run_that_established_every_dimension_is_assured`, the premise that the default can be satisfied |

The audit holds whether each column is a hole, and that its counts add up; the size of each count and the text of each named hole are the runner's, which the tests above hold.

## Context

A run now measures along more than one dimension: what a mutation changes ([ADR 0004](0004-proof-layers-not-budgets.md)), what a seam is asked ([ADR 0021](0021-a-claim-is-a-perturbation-an-observer-and-a-decision.md)), what a knob sets differently ([ADR 0031](0031-a-knob-is-one-control-started-differently.md)), and what a failed call does ([ADR 0032](0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
Schedules and durability were planned when this was written; both are measured now ([ADR 0034](0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md), [ADR 0035](0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)).
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
     Every record is placed as answered, a hole, or a class not spoken about by one exhaustive match over its decision, so a decision added later is one somebody places; `catalogued = answered + holes` holds by construction, and a count that would not fit is `unmeasured`.
     A class not spoken about is one no run could put, such as an error type the engine does not make; what this machine lacked and another could put, such as a locale that is not installed, is a hole.
     A column that holds records and none of them answered or open measured nothing, and is `unmeasured` rather than `measured` with a catalogue of zero; a tree with nothing to mutate, or a configuration that names no seam, is `nothing-to-ask`.
     Where a report holds several builds, a column is every build's counts added where each measured the dimension, and otherwise the column of the build that established least: a hole in any build is a hole of all of them.

2. **The matrix is derived, never stored.** Each column is computed from the records the part already holds: the mutation accounting, the knob records, the fault records, the seam records.
   A stored column would be a second copy of those records that a reader could find disagreeing with them.
   The record stream carries one `DIMENSION` record per column, and the drawing ends with the matrix.

3. **`whole-v1` asks every dimension, and a hole in any of them is not assured.** `whole-v1` runs the soundness phase as `deep-v1` does, puts every fault and every crash, sets every knob, and explores the schedules of every binary not proven to run one thread; the configuration layer does it, so every command reads the same configuration, and a document that says in so many words not to is refused rather than overridden.
   It concludes `INSUFFICIENT` whenever a column is `unmeasured` or `not-asked`, or is `measured` with a hole; `nothing-to-ask` and `speaks_not_about` are stated and are not holes.
   Each such column is a `dimension-not-measured` finding whose subject is the dimension's name, so the verdict is decided by findings as every other verdict is; a run of the whole catalog raises them, a shard raises none, and a merge raises them over every part.
   A report's conclusion derives them from the records every time and ignores the ones a part stored, so a report that drops one still names it.
   The schedule column makes this strict for any suite with threads: a binary is answered only when it is proven to run one thread (`-- --test-threads=1` over a closure that starts no thread) or a delay broke it, since a sample of schedules is a hole ([ADR 0034](0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md)), so an `INSUFFICIENT` `whole-v1` run is the contract answering rather than the tool failing, and the schedule row names which binary and why.
   A toolchain with no interpreter is a hole here where `deep-v1` refuses the run: `whole-v1` states `miri-unavailable` beside a `not-measured` finding, so the run is not `ASSURED` ([limitations](../limitations.md)).
   Every other contract reads the matrix and is decided exactly as it was.

4. **Contracts are answered by what they ask, never by comparing names.** The places that asked `contract == verified-v1` or `contract != deep-v1` now ask the contract what it runs — `runs_miri`, `proves_models`, `asks_every_dimension` — each an exhaustive match, so a contract added later is one the compiler makes somebody place.

5. **The default moves once every dimension is measured.** The plan makes `whole-v1` the default for a run that names no contract, and a default that nothing can satisfy is not a strict answer but a useless one.
   Every dimension is measured once schedules ([ADR 0034](0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md)) and durability ([ADR 0035](0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)) are, and the default moves to `whole-v1` in the change that measures the last of them; `standard-v1`, `deep-v1` and `verified-v1` stay what they are today, and a run that names one keeps it.

## Consequences

- A reader deciding whether to ship reads one table, and a dimension nobody asked about is a row saying so rather than an absence.
- `whole-v1` costs everything every dimension costs: a second instrumented build for faults, one control per knob per target, Miri over every crate with unsafe code.
- A dimension added later is a variant of `Dimension`, which the compiler makes the column, the record stream and the drawing name before anything builds.
