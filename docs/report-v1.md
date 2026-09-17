<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance report v1

**Status: implemented** (`njutest_cli::report`) — the model, its audit, and
all five projections.

The first public report contract is `njutest-assurance-report-v1`. The
schema value names the toolchain so a reader never confuses it with goatest's
`assurance-report-v1`, whose shape it shares.

## Durable layout

Each completed verification owns a directory that is immutable for as long as it
exists:

```text
reports/runs/<run-id>/
  njutest-assurance-report-v1.json
  njutest-assurance-report-v1.html
  njutest-assurance-report-v1.sarif
  njutest-assurance-report-v1.junit.xml
  njutest-assurance-report-v1.schema.json
```

The directory is `[reports] directory`, `reports` unless the configuration
says otherwise, and an index names a run the way somebody standing in the
project would: `jq -r .directory reports/latest-any.json` is a path to follow
from the project's own root.

`reports/latest-any.json` and `.njutest/latest-any.json` track the latest
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
- `rustc -vV` (version, commit hash, host), the cargo version, the njutest and
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

The JSON Schema is published at `schema/njutest-assurance-report-v1.json`
and copied into each run directory. Every object is closed with
`additionalProperties: false` and requires everything it declares, which is
what holds it to the model in both directions: a field the model gained and
the schema never heard of fails validation of a populated document, and a
field the schema declares and the model never writes fails because it is
required and absent. Rust validation
additionally enforces arithmetic, scope/verdict, acceptance, cache, and
unavailable-metadata invariants that JSON Schema alone cannot express
(`report::audit::validate_for_persistence`).

A **finding** is an actionable problem in the project or its verification
configuration. There are ten kinds, and a report carries the name rather
than a number, because the name is what a person greps for and what a
projection shows:

| `kind` | what it says | a defect |
| --- | --- | --- |
| `build-failure` | the workspace does not compile | yes |
| `failing-test` | a test of the workspace fails with nothing active | yes |
| `undefined-behaviour` | the interpreter found unsoundness where the compiler stops vouching | yes |
| `surviving-mutant` | every test that could notice a mutation passed with it active | no |
| `target-missing` | a test target could not be found, so nothing was observed about it | no |
| `timeout` | a target, or a mutation of one, ran out of the time it was given | no |
| `not-measured` | something a run could not measure, so it claims nothing about it | no |
| `unmatched-acceptance` | an unexpired acceptance does not name exactly one mutant in this catalog | no |
| `hollow-target` | a test target was put to mutations and noticed none of them | no |
| `wire-unnoticed` | the suite carried on through a question a seam licensed: a fault nothing noticed | no |

The last column is the one a reader acts on first: a defect is a fault in the
code under test, and the rest are gaps in what was established. Both are
findings, and a report that carries either is not an assurance.

Before mutation execution, every unexpired `[[acceptance]]` ID or prefix is
resolved against the session's complete catalog. Only a prefix that names
exactly one mutant is normalized to that mutant's full ID and may answer for a
survivor. An invalid, absent, or ambiguous prefix suppresses nothing and raises
one `unmatched-acceptance` finding whose `subject` is the configuration value.
An expired acceptance is ignored. A shard cannot independently audit this
relationship because it does not carry the complete catalog; the merged report
does, and its audit rechecks that every such finding still fails to resolve
uniquely.

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
recipe, so a recipe that changes says so: it reads `njutest-target-v2`.

A target's `duration_ms` is the cost of running every test it holds, once,
with nothing active. It is not divisible by the number of tests: a target that
takes a second for one test and a second for a hundred is two facts about
process starts and one fact about the tests. An estimate built from it may
decide an order and never a budget.

`targets` is canonically ordered by descending duration, then ascending target
ID. A mutant disposition may say `reused: true` with a `source_run_id`; the
accounting carries `reused_killed` and `reused_survived`, each part of
`killed` and `survived`.

## What a seam was asked

`seams` holds every question a watched seam's recording licensed and what
became of it: the question's `id`, the `capability` and the `seq` of the
exchange it is about, what was `asked` and what the upstream `answered` where
the wire says how to read one, the `rule` it applies, the `decision`, and
`noticed_by` — the target that noticed, or the proof that discharged it.

A `wire-unnoticed` finding names a question by its `id` and nothing else. A
reader handed a sixty-four character name with nothing to look it up in has
been told nothing they can act on, and [ADR 0002](adr/0002-trace-is-not-evidence.md)
keeps the thing it stands for off the recording, so it is here. The audit
refuses to persist a report whose seam finding names a question the report does
not hold.

`decision` is the same six-way partition the mutations use. `proved` is a
question no observer could have answered — cutting an answer with no body short
hands the caller the bytes it had — and `unreached` is a question the run could
not put, which is stated as a `not-measured` finding and never as a survivor.

## What the run observed the system doing

`njutest report --format spec` reads `seams` back as one sentence per exchange
and says who is holding each one up:

```text
POST /orders on payments answers 201 — held up by pkg/test/orders
GET /orders/1 on payments answers 200 — nobody holds this up; 5 question(s) nothing noticed
```

The sentences are made of what went past and nothing else, so they are always
true of the system as it ran. That is what makes the annotation the useful half:
the question is never whether the behaviour is real, only whether anybody would
notice it changing. A reviewer can be told, about a change to the second line,
that nobody was holding it up.

A question a proof discharged holds nothing up and leaves nothing wanting —
nobody could have noticed it, so counting it against the tests would ask them
for something no test can give. A question the run could not put is counted
apart from one nothing noticed, because writing a test is what closes the first
and there is nothing to write for the second.

## Who decided each mutation

The counts beside each other overlap — `executed` holds `killed` and
`survived` both, and `reused_killed` is part of `killed` — so they answer how
much of each kind of work a run did. `accounting.mutants.observers` answers a
different question, and its six columns **partition the catalog**: every
catalogued mutation is in exactly one of them, and
`report::audit::validate_for_persistence` refuses a report where they do not
add up to `cataloged`.

| column | what decided it | the outcome it comes from |
| --- | --- | --- |
| `types` | the compiler refused the program | `compile-rejected` |
| `tests` | a test noticed | `killed`, `timed_out` |
| `proved` | no test of any kind could have noticed | `equivalent` |
| `unnoticed` | it ran and nothing noticed | `survived` |
| `unreached` | nothing ran at all | `unreached` |
| `undecided` | nothing decided it | `unconfirmed`, `errored` |

`types` is the same number as `rejected`, said as what it is. A mutation the
compiler refuses is a program the type system would not let anybody have,
which is the same kind of event a failing test is: something noticed. Counted
only as work the run did not do, it is the one measurement nothing else in
the toolchain makes and this report used to discard. It changes no
denominator: `rejected` keeps its place in
`cataloged = rejected + executed + unreached + equivalent`.

`undecided` is a gap in the verification rather than in the project, and it is
never silent: a run that could not decide a mutation says so here and carries
the finding that explains it.

## Who could have noticed

`mutants[].routing` is what a survivor is a claim about. The assurance
contract's predicate — a mutation goes to the targets that reached it, less
the ones a proof discharged — decided every execution, and until now it was
visible only in the trace. [ADR 0002](adr/0002-trace-is-not-evidence.md) says
a trace is never evidence, so a reader holding a survivor to that predicate
was holding it to something the run does not answer for. The record answers
for it now.

| field | what it says |
| --- | --- |
| `granularity` | how narrowly the run chose: `all`, `block`, `test`, `discharged`, `unreached` |
| `reaching` | the targets that could have noticed it |
| `discharged` | the targets a proof removed, each with the proof that removed it |
| `fallback` | what widened the question, when the run could not narrow it: `not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, `touch-incomplete` |
| `answered` | the targets the run actually asked, in order, each with what it said |

`routing` is `null` where the run never asked, which is a mutation the
compiler refused: a program that does not exist is not one any test could
have noticed.

The difference between an empty `reaching` with a `discharged` list and an
empty one without is the difference a reader acts on. The first is a proof —
nothing could have noticed. The second is a hole — nothing looked.

`answered` is who was actually asked, in the order they were asked, and what
each said. `reaching` is who *could* have noticed; the two differ because a
run stops at the first detection and asks the cheapest targets first. A
target in `reaching` and absent from `answered` reached the mutation and was
never given the chance.

That distinction is the whole of why `answered` is recorded, and it is what
makes `hollow-target` sound. A target is executed against a mutation only
when every target earlier in the route already survived it — so every
execution that happened is one where that target had its chance and did not
take it. The early return is not an obstacle the finding works around; it is
the reason the finding is true.

A target absent from every `killed_by` is a different thing entirely and
proves nothing: a run records the first detection, so a target that is
outranked every time never appears there however sharp it is. A reader
settling "was this target put to mutations and did it notice none" from
`killed_by` would get it wrong; from `answered` they get it right.

## More than one build

`[[configuration]]` names further builds beyond the one `[execution]`
describes, which a report calls `default`. Two builds of a project are two
programs, so [ADR 0007](adr/0007-survived-evidence-is-universal.md)'s rule
that a kill is existential does not carry across them — that rule is about
one program measured twice. Across builds the quantifiers turn over: a
mutation nothing noticed in the release build is one nothing noticed,
whatever the debug build said, because the release build is a program
somebody ships.

So each mutation stands on **the weakest thing any build established**, in
the order `undecided`, `unnoticed`, `unreached`, `types`, `tests`, `proved`
— the first three being holes and the last three not. A run cannot come out
better for having looked at more.

`mutants[].blind_in` names the builds that are blind to a mutation: the ones
where nothing noticed it, nothing ran it, or nothing decided it. A gap under
one build and a gap everywhere are different things to act on. A run that
measured one build names none, because which build is not a question it has.

Builds that catalogued different mutations are refused rather than
reconciled. Taking the answers there are would report one build's silence as
agreement.

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

A run that kept a report writes a `REPORT` record naming the document it
wrote, so a script that took the verdict from the tail reads the rest beside
it without knowing where `[reports] directory` points. The composite action
does exactly that.

## Parts of one catalog

`njutest verify --shard K/N` judges one part of the catalog and measures the
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

`njutest merge <REPORT>...` writes the report the whole would have written. It
passes one already unsharded report through, or requires exactly one report for
every label `1/N` through `N/N`. It refuses a missing, repeated, malformed,
mixed unsharded, or differently divided part. It also
refuses parts that disagree about the tree, configuration, contract, effective
scope, or runner and engine versions, and parts that both judged one mutant.
Only that complete union is allowed to lose `scope.shard`: otherwise a missing
part could be mistaken for a catalog with no mutants in it. The mutant rows are
the union, the accounting is derived from that union rather than added up from
what each part claimed, and the verdict is decided again from the whole. A
score is a ratio and never survives a merge: two ratios over different
denominators average into a number no run observed.

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

`njutest --help` prints this table, and it prints it from the verdicts
themselves rather than from a copy: a run that has no verdict for a code has
no line for it. This page is held to what that prints.
