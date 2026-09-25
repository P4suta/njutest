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
| `environment-dependent` | a target that passed on its baseline failed on a control started with a knob put | yes |
| `environment-dependent-reach` | a target reached something else on a control started with a knob put, over the same passing tests | no |

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
and `not-measured` with one closed `why`: `no-control` (the run was cancelled before any control of it ran), `unrecorded`, `unreadable`, `control-failed`, `other-tests`, `no-baseline`, `baseline-retried`, which a target whose baseline passed only when run again in the directory its failed first attempt left gets, because that run did not happen under the conditions a control's does, or `unparsed`, where the tests either run was read as passing do not come to the count its own summary gives, because then which tests passed is the parser's answer and not the harness's.

A part that measured the whole catalog raises `unstable-baseline` about each moved target and states `drift-not-measured` naming every target that is not measured.
A shard records drift and raises neither, and concludes `INSUFFICIENT` rather than `PARTIAL` where a target moved; a merge raises both from the combined records of every part of the build.
The same holds for `hollow-target`, since which targets answered about a mutation and noticed none is only known over the whole catalog.
What only the whole catalog decides is one function, `report::whole_catalog`, called by a run that measured the catalog whole and by a merge over the combined records, so a catalog concludes the same whether it was measured whole or in shards.
Re-executing what rested on a moved record is not done by this release; the finding is what a reader acts on.

## Knobs

Every part carries `knobs`, one record per knob the configuration asked for and per target whose baseline passed, each `{ target, knob, standing }`: what one more control of that target, started with one thing the contract lets differ between machines set differently, established against the baseline.
`knob` is one of `timezone`, `locale`, `temp-directory`, `home`, `umask`, `columns`, `threads`.
`standing` is closed by its `state`:
`stable` (it passed the tests its baseline passed and reached the same three unions drift compares),
`passed` (it passed, and is a target that records no reach to compare, as a doctest run through cargo is),
`broke` with the tests that `failed`,
`moved` with the `reach` it gained and lost in each union,
`uncompared` with drift's closed `why`,
`unsettled` with `errored` or `waited`,
and `not-put` with `why`: `platform`, `zone-missing`, `locale-missing`, `shell-missing`, `through-cargo`, or `not-libtest`.
A part whose records repeat a knob for a target, or put two knobs on different targets, is refused, since every knob asked for is put once on every passing target.

A part that measured the whole catalog raises `environment-dependent`, a defect, about each target a knob broke, `environment-dependent-reach` about each whose reach a knob moved, counting what rests on its baseline by the rule `unstable-baseline` counts with, and states `knob-not-put` and `knob-not-compared`.
A shard records its knobs and raises none of them, and concludes `INSUFFICIENT` rather than `PARTIAL` where a knob broke or moved a target; a merge raises them from the combined records of every part, keeping of two records of one knob and target the one that says more.

## Sources

Every part carries `sources`, one `{ path, digest }` per file its mutants were read from, in path order: the SHA-256 of the file's bytes as the run read them, taken from the catalog, which already refuses two digests for one file.
It is what lets a reader of the report tell the file the run measured from the file there now.
Every surface that quotes source code — the terminal page, the review loop, the briefing, the language server — draws a line only from a file whose SHA-256 now is the one recorded; a file edited since the run is said to have changed, never drawn as though it were the code the run measured, even when the edited line still holds the text the run replaced.
The language server places nothing in such a file and says instead which run measured it.
A file that cannot be drawn says why, because each why is a different thing to do: it has changed since the run, it is not there any more, it could not be read for the reason reading it gave, or it holds the bytes the run read and they are not text.
A carriage return a Windows checkout left is the checkout's and not the line's, so no surface draws one.

A row or finding naming a file with no entry is refused, and so are two parts or builds of one run that recorded different digests for one file, since then they did not read one tree.
A document that writes a path twice, or out of path order, is not read: a file has one digest, and a document has one spelling of it.

## How wide a run measured

`run.jobs` says how wide the run measured: `asked`, as a person writes it — a count, `auto` (the machine, capped at four), or `all` (every processor) — and `used`, how many mutants were measured at once.
The engine resolves the width once, runs at it, and writes that value, so the report and the run cannot disagree; the report's lines print it as `jobs      <used> (<asked>)`, so a CI log says how wide the run was without anybody opening the report.

## Shards and projections

A `K/N` shard owns dense catalog indices whose index modulo `N` is `K - 1`.
A part concludes `PARTIAL`; only a complete, non-overlapping set of all parts can be merged into an unsharded verdict.
The merge re-derives accounting,
findings and verdict from the union instead of adding claims from the parts.
A run judges an expectation only on the mutations it decided: not those another part holds, a selection such as `--file` left out, or a stop came before.
A change set (`--changed`, `--changed-from`) builds the catalog from the files it names alone, so a claim on another file resolves to nothing there; it too is `unjudged`, while a claim on a file the change set kept that names nothing is still `unmatched`.
One that decided none of them says `unjudged`, which is neither met nor contradicted and earns no finding, so a run over one file is not failed by claims about another.
A `count` spread across parts is therefore checked by the parts together;
each resolves the claim against the whole catalog, so `covered` is the whole claim's count in every part, and a claim that resolves to nothing is `unmatched` alike in every part.
Every standing is a statement about each of the claim's mutations — every one of them has the claimed outcome — and the whole run names the first mutation in catalog order that contradicts it, or the first when none does.
So the merged expectation is the part's answer naming the earliest contradicting mutation, or failing any, the earliest met one, and is `unjudged` only where no part decided any of them; a form that counted or asked for "at least one" would need its own merge, and is not one of these.
The merge first puts its reports in shard order and refuses a set that is not every part of one catalog, each once, naming the part that is missing or repeated, so the merged document does not depend on the order the reports were offered in.
A `stale-expectation` or `unmatched-expectation` finding is derived from its expectation, in a part and in a merge alike, and a document whose findings of those kinds are not exactly the ones its expectations earn is not read.

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
