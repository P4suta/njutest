<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance report v1

**Status: implemented** (`mjutest_cli::report`) — the model, its audit, and
all five projections.

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

A **finding** is a claim about the project. There are seven kinds, and a
report carries the name rather than a number, because the name is what a
person greps for and what a projection shows:

| `kind` | what it says | a defect |
| --- | --- | --- |
| `build-failure` | the workspace does not compile | yes |
| `failing-test` | a test of the workspace fails with nothing active | yes |
| `undefined-behaviour` | the interpreter found unsoundness where the compiler stops vouching | yes |
| `surviving-mutant` | every test that could notice a mutation passed with it active | no |
| `target-missing` | a test target could not be found, so nothing was observed about it | no |
| `timeout` | a target, or a mutation of one, ran out of the time it was given | no |
| `not-measured` | something a run could not measure, so it claims nothing about it | no |

The last column is the one a reader acts on first: a defect is a fault in the
code under test, and the rest are gaps in what was established. Both are
findings, and a report that carries either is not an assurance.

A **limitation** is the opposite: a claim the run declines to make about
itself. The audit holds the verdict and the findings to each other,
because a report must not say two things at once: an assurance is the claim
that nothing was found, so it carries no findings, and a `DEFECT` a reader
cannot see named is not one they can act on, so it carries at least one.

A target is a test binary, and its identity is the digest of the package, the
kind, the binary's name, and — for the shape a later release may take — the
libtest path within it. The binary is part of it because two integration tests
of one package can each hold a test called `works`, and an identity that left
the binary out made those two rows one. The domain separator carries the
recipe, so a recipe that changes says so: it reads `mjutest-target-v2`.

A target's `duration_ms` is the cost of running every test it holds, once,
with nothing active. It is not divisible by the number of tests: a target that
takes a second for one test and a second for a hundred is two facts about
process starts and one fact about the tests. An estimate built from it may
decide an order and never a budget.

`targets` is canonically ordered by descending duration, then ascending target
ID. A mutant disposition may say `reused: true` with a `source_run_id`; the
accounting carries `reused_killed` and `reused_survived`, each part of
`killed` and `survived`.

## What a finding is about

A finding names its `subject` — a mutant, a target, a package — and an
identity is not somewhere anybody can open. So a finding that is about a
place in the tree also carries `path`, relative to the workspace root, beside
its `position`. Both are `null` for a finding that is about a target or a
package rather than a line.

Every consumer that puts a finding on a line needs the file as well: SARIF
shows an alert against the path in the log, and a log that gave the identity
instead put every alert on a file nobody had.

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

Terminal and pipe output is one record per line, tab-separated, the kind
first and the verdict last, so `tail -1` is the answer and a filter on the
first field is a projection. Every value the run did not write itself — a
test's failure message, a provider's diagnostic, a limitation's detail — is
escaped: a newline, a tab, a carriage return, and a terminal escape are
spelled out rather than emitted. Without that, output from the code under
test could forge a `FINDING`, `REPAIR`, `ACCEPTANCE`, or `LIMITATION`
record, and a reader filtering for one would read a claim the run never
made.

## Parts of one catalog

`mjutest verify --shard K/N` judges one part of the catalog and measures the
whole baseline, because a mutation cannot be judged against tests that were
not run. The engine's rule decides which part holds which mutant — the dense
catalog index modulo N, counting K from one — so two runs of the same tree
divide it the same way without talking to each other, and every mutant belongs
to exactly one part. Nothing is sampled and nothing is skipped, which is what
keeps this out of [ADR 0004](adr/0004-proof-layers-not-budgets.md)'s way: it
divides the work rather than reducing it.

A part concludes `PARTIAL` and records its `scope.shard`. It assures nothing on
its own: the mutations it did not judge are not mutations nothing noticed, they
are mutations nobody put to a test. A finding in a part is still a finding, so
a part that found a defect says `DEFECT`.

`mjutest merge <REPORT>...` writes the report the whole would have written. It
refuses parts that disagree about the tree, the configuration, or the contract,
and parts that both judged one mutant — the last says they were cut with
different values of N. The mutant rows are the union, the accounting is derived
from that union rather than added up from what each part claimed, and the
verdict is decided again from the whole. A score is a ratio and never survives
a merge: two ratios over different denominators average into a number no run
observed.

Every run's identity carries its shard, so a part never reads back the whole's
stored answer and a whole never reads back a part's.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `PARTIAL`, `RESOLVED` |
| 1 | `DEFECT`, `REPRODUCED` |
| 2 | `INSUFFICIENT` |
| 3 | `ERROR`, invalid input, or an infrastructure failure |
| 130 | interrupted |
| 143 | terminated |

`mjutest --help` prints this table, and it prints it from the verdicts
themselves rather than from a copy: a run that has no verdict for a code has
no line for it. This page is held to what that prints.
