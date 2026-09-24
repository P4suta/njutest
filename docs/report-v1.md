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
A finding is one of four derivations, by its kind (`FindingKind::derivation`).
A mutation row decides the surviving, waited and step-limit ones; the part's own records decide the ones they are raised from — `hollow-target`, `unstable-baseline`, `unnoticed-fault`, `corrupt-after-crash`, `environment-dependent`, `environment-dependent-reach`, `schedule-dependent`, `dimension-not-measured`; `not-measured` is raised both from records and by phases; and the rest a phase observed.
A report is refused, when it is written and when it is read, if the findings its records decide are not exactly the ones it holds: one no record raises, or one they raise that it dropped, is a report saying something its records contradict.

There are twenty kinds, and every report carries the stable name:

| `kind` | what it says | a defect |
| --- | --- | --- |
| `build-failure` | the workspace does not compile | yes |
| `failing-test` | a test fails with nothing active | yes |
| `undefined-behaviour` | the interpreter found unsoundness | yes |
| `corrupt-after-crash` | the next run failed over what a stop just after a call that writes left, with the stopped test among its failures, and three rounds of a fresh run that passes and another stop that fails it the same way confirmed it | yes |
| `broken-under-fault` | one fault, run alone, wrote into the tree under measurement, and the same test run alone without it did not | yes |
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
| `dimension-not-measured` | a `whole-v1` run did not establish the dimension its subject names | no |
| `environment-dependent` | a target that passed on its baseline failed on a control started with a knob put | yes |
| `environment-dependent-reach` | a target reached something else on a control started with a knob put, over the same passing tests | no |
| `schedule-dependent` | a test binary failed with one guard delayed, twice more, and passed without the delay | yes |

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
Every `unnoticed` record raises one `unnoticed-fault` finding naming its `display_id`, and every `waited` or `undecided` one a `not-measured` finding naming it, so a run asked for faults that could not decide one is not `ASSURED`.
`not-put` records are stated as one `fault-not-put` limitation naming each compiler error class with its sites.
A tree whose faulted baseline could not be measured raises a `not-measured` finding about `fault-baseline-not-measured` and carries no records.
A record's `position` is `null` where the run could not place the site.
A path of the tree written while faults were put, and not before any was, is a `not-measured` finding about `fault-write-unattributed` naming it, since no one execution is tied to the write; `broken-under-fault` is kept for a write tied to one fault.

Every part also carries `beside`: one record for each error-propagation survivor of the part (`question-to-unwrap`, `ignore-question-statement`) that a target told apart once the call at its own `?` failed ([ADR 0032](adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md) decision 6).
The survivor is put again with the fault that is carried into its alternative active beside it, target by target in name order, and the fault alone is put to the same target; a record names the first target on which exactly one of the two runs failed, and `failed` says which (`beside` or `alone`), confirmed by running the survivor beside the fault a second time.
It is evidence that the survivor is not an equivalence and never a kill: the survivor stays `survived`, its finding stays, and no count moves, because no test made that call fail.
A record names a survivor the part holds, and in a part of the whole catalog a fault it holds too; a shard owns its survivors by their index, and the fault beside one may be in another shard.
Only a fault whose guard the instrumentation carried into the survivor's branch is ever put beside it: an `ignore-question-statement` rewrites the whole statement, above the node the call's fault sits at, and is not asked.

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
## Concurrency

Every part carries `concurrency`, one `{ target, standing }` per test binary the run measured, in binary order ([ADR 0034](adr/0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md)).
`standing` is closed by its `state`:
`single-threaded` where the binary's baseline reached nothing off its tests' threads and no package of its closure can start a thread or runs native code;
`concurrent` with every reason that holds in `because`, each `loose-reach`, `parallel-tests` (libtest ran its tests on more than one thread, which it does unless the run passes `--test-threads=1`), or `starts` with the `package`, `path`, `line`, and `what` (`spawn`, `scope`, `parallel`, `runtime`);
and `not-proven` with every reason in `why`: `no-touch`, `not-libtest`, `doctest`, `unread` with the `package` and `path`, or `native-code` with the `package` and `by` (a `path:line`, or `links`).
`explored` says what delaying its guards found, closed by `state`: `unexplored` with `why` (`not-needed` for a single-threaded binary, `not-asked`, `not-passing`, `no-site`), `sampled` with the number of guards `asked` for and every guard `delayed` where each delayed control passed, `undecided` with `asked`, `delayed`, and those whose controls settled nothing or whose failure no round confirmed as `undecided`, or `broke` with the `site`, its `path` and `line`, the tests that `failed`, and the confirming `rounds`.
A delay broke a binary only where, in each of five rounds, a delayed control failed exactly the same tests and an undelayed one passed; the part then raises `schedule-dependent` about it, a defect, and a shard explores nothing.
A part whose records name a binary twice or out of order is refused.

A part states `schedule-not-explored` naming every binary that is not `single-threaded`: no schedule is explored yet, so each is a hole.

## Crashes

Every part carries `crashes`, one record per call that writes a crash was asked at, in catalog order, and empty unless the run was asked for crashes ([ADR 0035](adr/0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)).
A crash site is a call that writes in a measured file, discovered by the rule `crash-after-write` alone; under it the call runs and the process stops at once.
The first test that reaches the call, target by target in name order, is stopped there and run again with nothing active in the scratch the stop left, and each record carries one closed `decision`:

| `decision` | what it says | carries |
| --- | --- | --- |
| `restarted` | the next run passed over the files the stop left | `on`, the target and test; `left`, those files |
| `corrupt` | the next run failed, a fresh run passed, and a second stop failed the next run again | `on`; `failed`, the tests |
| `unshared` | the stopped run left nothing in its scratch | `on` |
| `unreached` | no test that reached the call stopped at it | |
| `undecided` | a run came to something other than passing or stopping at the call, which test reaches the call is not known, or an earlier stop wrote outside its scratch into the tree | `on`, `why` |
| `not-put` | the compiler refused the crash | `diagnostic` |

`accounting.crashes` counts the records, and `restarted + corrupt + unshared + unreached + undecided + not_put` equals `sites`.
Every `corrupt` record raises a `corrupt-after-crash` finding, a defect; every `unshared` or `undecided` one a `not-measured` finding.
`restarted` says the next run passed over what the stop left, not that it read it, and a stop here is a process stopping, not the power failing; the durable column says it does not speak about either.
A tree with no call that writes in a measured file states `crash-no-site`, and one whose crashed baseline could not be measured raises a `not-measured` finding about `crash-baseline-not-measured`.

## The matrix

A report is read along six dimensions, `mutation`, `repeatable`, `fault`, `schedule`, `wire` and `durable` ([ADR 0033](adr/0033-every-dimension-or-a-hole.md)).
The matrix is derived from the records above and never stored: a stored column would be a second copy of them a reader could find disagreeing.
Each column is `measured` with `catalogued`, `answered`, `holes` (which add up) and what it `speaks_not_about`, or `unmeasured` with why, `not-asked`, or `nothing-to-ask` with why.
Mutation holes are the waited, step-limited, unconfirmed and errored mutations; knob holes the uncompared and unsettled records, and knobs not put are what it does not speak about; fault holes the waited and undecided sites, and sites not put are what it does not speak about; wire holes the questions not reached and the seams not watched, and it never speaks about a seam the configuration does not name.
The record stream carries one `DIMENSION` record per column.
Under `whole-v1`, every column that is not `measured` without a hole or `nothing-to-ask` is a `dimension-not-measured` finding whose subject is the dimension's name; a run of the whole catalog raises them, a shard raises none, and a merge raises them over every part.

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
