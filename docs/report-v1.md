<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance report v1

**Status: implemented.** Current runs write `njutest-assurance-report-v1`; the v1 page and schema describe historical artifacts only.
The canonical JSON Schema is `schema/njutest-assurance-report-v1.json`, and every object it declares is closed.
Rust deserialization additionally enforces relationships that JSON Schema cannot express, including the exact `limit + 1 == observed` step boundary.

Every closed set the schema declares is held to the names this release produces, in both directions.
The store boundary already refused a report carrying a name the schema does not admit, so that direction failed in every run that produced one; the other failed in no run at all, and a name the schema admitted that nothing emits is a branch a consumer writes and never reaches.
`docs_ledger::every_closed_set_the_schema_declares_is_one_this_release_produces` reads the schema's thirty-six `enum` and `const` sets and compares each against the Rust set that produces it — `Outcome`, `FindingKind`, `Blind`, `Fallback`, `Granularity`, `Proof`, `Contract`, the nine `Model*` sets, and the rest — so a set the schema gains and nothing on this side answers is a refusal rather than a row nobody reads.

Each completed run owns an immutable directory under `[reports].directory`.
The canonical document is `njutest-assurance-report-v1.json`; HTML, SARIF,
JUnit and line output are projections of that document.
`latest-any.json` names the latest completed run, and `latest-full.json` advances only for a full run.

Authoritative report publication and reading currently require the Unix handle-relative filesystem backend.
That backend holds the workspace,
configured report root, selected run, and each file open while it validates and uses them; it never turns a checked path spelling back into authority.
On Windows and other non-Unix hosts, commands that would publish, select,
read, retain, or delete durable reports refuse with the typed `REPORT_NOT_KEPT` error.
They do not fall back to pathname checks whose object could change between validation and use.

## Mutation records

`outcome`, `killed_by`, and `step_boundary` are one typed value in Rust and one closed schema arm on the wire.
A kill names its target.
A `step-limit-reached` record names the target and carries a nonzero allowance plus exactly its first excluded count.
`waited`, `unconfirmed`, and `errored` name the target on which no verdict was established.
Outcomes that did not occur on a target name none.
A boundary on any other outcome, or a step-limit outcome without one, is not a v1 document.

`step-limit-reached` is an execution fact, not a detection.
It is a hole in the verification, contributes to neither side of a score, is never cached or checkpointed, and cannot be answered by an acceptance.
Historical v1 `runaway` records have no matched control and are never reinterpreted as v1.

Every mutation row also carries an explicit `accepted` boolean.
This makes the durable `accounting.mutants.accepted` column independently derivable;
`true` is valid only for `survived`, `unreached`, or `equivalent` rows.

`blind_in` names each build in which a mutation remains a hole, using exactly `unnoticed`, `unreached`, `step-limit-reached`, `waited`, or `errored`.
Answered builds cannot be represented in that field.

## Findings

A **finding** is an actionable defect or an explicit gap in what the run established.
There are fifteen kinds, and every report carries the stable name:

| `kind` | what it says | a defect |
| --- | --- | --- |
| `build-failure` | the workspace does not compile | yes |
| `failing-test` | a test fails with nothing active | yes |
| `undefined-behaviour` | the interpreter found unsoundness | yes |
| `broken-under-fault` | a test wrote into the tree under measurement while a fault failed a call, which it did not do while none did | yes |
| `surviving-mutant` | every reaching test passed with the mutation active | no |
| `target-missing` | a selected target could not be measured | no |
| `timeout` | a non-mutation phase exhausted its time bound | no |
| `waited-mutant` | a mutation execution exhausted its wall-clock bound | no |
| `step-limit-reached-mutant` | a verified finite step boundary was crossed without a matched control verdict | no |
| `not-measured` | the run could not make the stated measurement | no |
| `unmatched-acceptance` | an active acceptance names other than exactly one catalog entry | no |
| `hollow-target` | a target was put to mutations and noticed none | no |
| `wire-unnoticed` | a seam fault was put and nothing noticed | no |
| `unstable-baseline` | a target reached something on an original-code control that it did not reach on its baseline, over the same passing tests | no |
| `unnoticed-fault` | a call a `?` asks about failed and every test that reached it passed | no |

The last column is derived from the same closed `FindingKind` that decides the verdict.
A report with a defect concludes `DEFECT`; a report with only gaps concludes `INSUFFICIENT`; an assurance carries no findings.

## Who decided each mutation

`accounting.mutants.observers` partitions the catalog.
There are ten columns,
in the same closed order as `Decision::ALL`:

| column | what decided it | the outcome it comes from |
| --- | --- | --- |
| `types` | the compiler refused the program | `compile-rejected` |
| `tests` | a test noticed | `killed` |
| `model-noticed` | the model checker produced a distinguishing input | `model-noticed` |
| `model-proved` | the model checker proved equality throughout the closed domain | `model-proved` |
| `proved` | no observer could distinguish the programs | `equivalent` |
| `unnoticed` | every reaching test ran and none noticed | `survived` |
| `unreached` | no measured target reached the mutation | `unreached` |
| `step-limit-reached` | a verified execution boundary was crossed, without a verdict | `step-limit-reached` |
| `waited` | the wall-clock bound expired before completion | `waited` |
| `errored` | no verdict could be established | `unconfirmed`, `errored` |

The accounting is re-derived exactly from the ID-level records.
Mutation and target identities are unique, target rows are in canonical order, every target status column is reproduced from the rows, and every mutation outcome,
reuse, and acceptance column equals the row-derived count.
In particular:

```text
cataloged = rejected + executed + unreached + equivalent
executed >= killed + survived + step_limit_reached + waited
accepted <= survived + unreached + equivalent
reused_killed <= killed
reused_survived <= survived
observers.total() = cataloged
```

Only killed and survived mutation evidence is reusable.
Model answers retain their generated source, raw export, process termination, hashes, pinned tool and backend identity so an independent audit can re-derive the affirmative answer rather than trusting a summary.

Every model identity also carries one closed `crate_input` object.
Its package is `njutest-verified-model`, edition is `2024`, source is `src/lib.rs`, network policy is offline, and `dependency_resolution` is exactly `empty-lock-offline-v1`: the dependency-free manifest, exact empty lockfile,
offline execution, and pre/post whole-crate byte check replace a Cargo flag which Kani 0.68 does not accept.
Its `environment` is `minimal-v1`: the verifier receives a newly constructed environment containing only the fixed Cargo and Kani homes, checker path, private target/temp paths,
explicit target, offline mode, and empty compiler-wrapper/flag slots.
Caller Python, toolchain, compiler, wrapper, Nix, and dynamic-loader variables are not inherited.
Its SHA-256 is a domain-separated digest over that policy, the exact fixed `Cargo.toml`, fixed `Cargo.lock`, and retained generated source.
Rust deserialization checks that digest against the rendered-source digest; `modelaudit` independently reads the source and recomputes the same crate input from its own constants.
Kani therefore cannot earn an affirmative row from an unreported subject manifest, dependency,
build script, target, or stale lock resolution.

This evidence pins and rechecks observable verifier identities; it is not a software-supply-chain attestation for the host.
A local actor able to replace the checked `cargo-kani`, Cargo/rustup proxy, Kani bundle, operating system, or files between validation and execution remains inside the documented machine trust boundary.

## Model records

Every `models[]` entry has exactly two outer fields: the full `mutant` identity and a nested `answer`.
The nested value is one closed arm:
`ineligible { decision, reason }`, `undecided { decision, attempt: { reason,
evidence } }`, or affirmative `noticed`/`proved { decision, evidence }`. Keeping the answer behind an explicit field prevents a decision arm from merging its namespace with the record identity, and both the schema and `proofaudit` reject unknown fields at either level.

Under `verified-v1`, every `survived`, `model-noticed`, and `model-proved` mutation row has exactly one model record.
Conversely, every model record names exactly one mutation row, and its answer agrees with that row;
duplicates and missing counterparts are rejected.
`standard-v1` and `deep-v1` do not run this model phase, so both retained model records and model-decided mutation outcomes are invalid under those contracts.

## Routing

`mutants[].routing` carries the premise of every target-level conclusion:

| field | what it says |
| --- | --- |
| `granularity` | `all`, `block`, `test`, `discharged`, or `unreached` |
| `reaching` | targets that could notice the mutation |
| `discharged` | targets removed by `branch-never-taken` or `never-infected` |
| `fallback` | why routing widened: `not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, or `touch-incomplete` |
| `answered` | targets actually asked, in order, with their outcomes |

A row this run decided by a route it asked is held to that route's own answers, and a report that contradicts them is refused.
A `killed` row's answers end with the target it names noticing, and hold no other kill: the mutation phase stops at the first target that notices.
A `survived` row's answers hold one survival from each target in `reaching` and nothing else, in whatever order the run asked them.
A row read back from another run, or inherited from a checkpoint without a route, was not asked here, so the rule has nothing to hold it to.
For a kill this run established, `by` is therefore a second copy of the last answer's target; the rule keeps the two in step until a later schema stops storing both.

Evidence consultation records either the source run it reused or one closed refusal: `nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`,
`key-changed`, `not-passing`, `target-entered`, or `nothing-routed`.

## Drift

Every part carries `drift`, one record per target whose baseline recorded what it reached, because every proof of the part is read off that record ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)).
A record's `state` is `held` where an original-code control of the whole target that passed exactly the tests the baseline passed reached the same three unions — sites, bodies entered, sites infected —
`moved` where it did not, with what only the control reported (`gained`) and what only the baseline reported (`lost`) for each union by catalog index,
and `not-measured` with one closed `why`: `no-control` (nothing confirmed a kill on it), `unrecorded`, `unreadable`, `control-failed`, `other-tests`, `no-baseline`, `baseline-retried`, which a target whose baseline passed only when run again in the directory its failed first attempt left gets, because that run did not happen under the conditions a control's does, or `unparsed`, where the tests either run was read as passing do not come to the count its own summary gives, because then which tests passed is the parser's answer and not the harness's.

A part that measured the whole catalog raises `unstable-baseline` about each moved target and states `drift-not-measured` naming every target that is not measured.
A shard records drift and raises neither, and concludes `INSUFFICIENT` rather than `PARTIAL` where a target moved; a merge raises both from the combined records of every part of the build.
Re-executing what rested on a moved record is not done by this release; the finding is what a reader acts on.

## Faults

Every part carries `faults`, one record per site a fault was asked at, in catalog order, and empty unless the run was asked for faults ([ADR 0032](adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
A fault site is a `?` in a measured file; its catalog is its own, discovered by the rule `inject-error` alone, so `catalog_index` counts faults and a `K/N` shard owns the faults whose index modulo `N` is `K - 1`, exactly as it owns mutations.
A record carries the fault's `id` and `display_id`, its `path`, `item` and `position`, and one closed `decision`:

| `decision` | what it says | carries |
| --- | --- | --- |
| `noticed` | a test failed with the call failing and passed on the unchanged program, and failed again on a second run | `by`, the first target in target order that noticed |
| `unnoticed` | every test that reached the site passed with the call failing | |
| `unreached` | no test reached the site | |
| `waited` | a bound expired with the call failing before a test finished | `on` |
| `undecided` | a test failed with the call failing and the run could not confirm it, or could not run the test | `on`, `why` |
| `not-put` | the compiler refused the fault, because the site propagates an error type the engine does not make | `diagnostic`, the compiler's first line |

Nothing here is a kill, and nothing is proved: a fault changes what the program is given, never the program, and no discharge is applied to a fault's route.
`accounting.faults` counts the records, and `noticed + unnoticed + unreached + waited + undecided + not_put` equals `sites`; a part whose counts do not is not a v1 document.
Every `unnoticed` record raises one `unnoticed-fault` finding naming its `display_id`.
`not-put` records are stated as one `fault-not-put` limitation naming each compiler error class with its sites, and `waited` and `undecided` ones as one `fault-not-decided` limitation.
A tree whose faulted baseline could not be measured states `fault-baseline-not-measured` and carries no records.

## Sources

Every part carries `sources`, one `{ path, digest }` per file its mutants were read from, in path order: the SHA-256 of the file's bytes as the run read them, taken from the catalog, which already refuses two digests for one file.
It is what lets a reader of the report tell the file the run measured from the file there now.
Every surface that quotes source code — the terminal page, the review loop, the briefing, the language server — draws a line only from a file whose SHA-256 now is the one recorded; a file edited since the run is said to have changed, never drawn as though it were the code the run measured, even when the edited line still holds the text the run replaced.
The language server places nothing in such a file and says instead which run measured it.
A file that cannot be drawn says why, because each why is a different thing to do: it has changed since the run, it is not there any more, it could not be read for the reason reading it gave, or it holds the bytes the run read and they are not text.
A carriage return a Windows checkout left is the checkout's and not the line's, so no surface draws one.

A row or finding naming a file with no entry is refused, and so are two parts or builds of one run that recorded different digests for one file, since then they did not read one tree.
A document that writes a path twice, or out of path order, is not read: a file has one digest, and a document has one spelling of it.

## Shards and projections

A `K/N` shard owns dense catalog indices whose index modulo `N` is `K - 1`.
A part concludes `PARTIAL`; only a complete, non-overlapping set of all parts can be merged into an unsharded verdict.
The merge re-derives accounting,
findings and verdict from the union instead of adding claims from the parts.

JSON is canonical.
Terminal output is tab-separated with the record kind first and verdict last; untrusted text is escaped.
HTML, SARIF and JUnit carry the same audit identity and findings.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `PARTIAL`, `RESOLVED` |
| 1 | `DEFECT`, `REPRODUCED` |
| 2 | `INSUFFICIENT`, `INCONCLUSIVE` |
| 3 | `ERROR`, invalid input, or an infrastructure failure |
| 130 | interrupted |
| 143 | terminated |

`njutest --help` renders the same table from the verdict types; the ledger test holds this page to that output.
