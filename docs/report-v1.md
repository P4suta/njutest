<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance report v1

**Status: the model, its audit, the JSON projection, and the published
schema are implemented** (`mjutest_cli::report`). Lines arrive later in M2;
HTML, SARIF, and JUnit in M3.

The first public report contract is `mjutest-assurance-report-v1`. The
schema value names the toolchain so a reader never confuses it with goatest's
`assurance-report-v1`, whose shape it shares.

## Durable layout

Each completed verification owns a directory that is immutable for as long as it
exists:

```text
reports/runs/<run-id>/
  mjutest-assurance-report-v1.json
  mjutest-assurance-report-v1.html
  mjutest-assurance-report-v1.sarif
  mjutest-assurance-report-v1.junit.xml
  mjutest-assurance-report-v1.schema.json
```

`reports/latest-any.json` and `.mjutest/latest-any.json` track the latest
completed run of any scope. `latest-full.json` exists in both locations and
advances only when `run_kind` is `full`. The history is bounded by
`[reports] keep`, twenty by default, plus the runs the `latest-*` indexes
point at. Nothing ever rewrites a run directory.

## Required audit identity

A durable report must include:

- schema, run ID, run kind, verdict, contract, and snapshot identity;
- requested and resolved workspace/package/file scope;
- repository package inventory and explicit Git availability, commit, dirty
  state, merge base, and changed files;
- an effective configuration SHA-256;
- `rustc -vV` (version, commit hash, host), the cargo version, the mjutest and
  rust-mutants versions, OS, architecture, and target triple;
- RFC3339 start/finish times and duration;
- cache-derived state and source run ID when applicable;
- target, soundness, and mutant accounting;
- every selected baseline target with its terminal status and measured
  `duration_ms`;
- every ID-level mutant disposition;
- acceptance metadata, evidence, findings, repair candidates, and structured
  limitations.

If Git is unavailable, the report uses the explicit `available=false` state
and `unavailable` sentinels together with `git-metadata-unavailable`; an empty
value is invalid.

The JSON Schema is published at `schema/mjutest-assurance-report-v1.json`
and copied into each run directory. Every object is closed with
`additionalProperties: false` and requires everything it declares, which is
what holds it to the model in both directions: a field the model gained and
the schema never heard of fails validation of a populated document, and a
field the schema declares and the model never writes fails because it is
required and absent. Rust validation
additionally enforces arithmetic, scope/verdict, acceptance, cache, and
unavailable-metadata invariants that JSON Schema alone cannot express
(`report::audit::validate_for_persistence`).

`targets` is canonically ordered by descending duration, then ascending target
ID. A mutant disposition may say `reused: true` with a `provenance`; the
accounting carries `reused_killed` and `reused_survived`, each part of
`killed` and `survived`.

## Positions

Every position in a report — a mutant's, a coverage region's, a finding's —
is a 1-based `line` with **two** 1-based columns: `column`, counted in UTF-8
bytes, and `character_column`, counted in Unicode scalars.

Two are carried because one toolchain uses both and neither is a safe
default. Measured on `fixtures/fixture-unicode`: an `llvm-cov` coverage
region's columns are **bytes**, and a rustc diagnostic's columns are
**characters**. A report that carried one unit would make every consumer
guess which, and a consumer that guessed wrong would point at the wrong
place in exactly the files where it matters. Both are derived from the same
byte offset, so they cannot disagree.

## Projections

JSON is the canonical complete model. HTML is self-contained and provides
scope/accounting/audit tables, a slowest-first target table, and client-side
search and section filtering. SARIF carries findings and the audit model in
run properties. JUnit represents evidence as passing cases, findings as
failures, and embeds core identity as properties.

Terminal and pipe output is deterministic and escapes control characters so
provider or test output cannot forge `FINDING`, `REPAIR`, `ACCEPTANCE`, or
`LIMITATION` records.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `RESOLVED`, or `COMPLETED` |
| 1 | `DEFECT` or `REPRODUCED` |
| 2 | `INSUFFICIENT` |
| 3 | `ERROR`, invalid input, or infrastructure failure |
| 130 | interrupted |
| 143 | terminated |
