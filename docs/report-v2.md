<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance report v2

**Status: implemented.** Current runs write
`njutest-assurance-report-v2`; the v1 page and schema describe historical
artifacts only. The canonical JSON Schema is
`schema/njutest-assurance-report-v2.json`, and every object it declares is
closed. Rust deserialization additionally enforces relationships that JSON
Schema cannot express, including the exact `limit + 1 == observed` step
boundary.

Each completed run owns an immutable directory under `[reports].directory`.
The canonical document is `njutest-assurance-report-v2.json`; HTML, SARIF,
JUnit and line output are projections of that document. `latest-any.json`
names the latest completed run, and `latest-full.json` advances only for a
full run.

Authoritative report publication and reading currently require the Unix
handle-relative filesystem backend. That backend holds the workspace,
configured report root, selected run, and each file open while it validates
and uses them; it never turns a checked path spelling back into authority.
On Windows and other non-Unix hosts, commands that would publish, select,
read, retain, or delete durable reports refuse with the typed
`REPORT_NOT_KEPT` error. They do not fall back to pathname checks whose object
could change between validation and use.

## Mutation records

`outcome`, `killed_by`, and `step_boundary` are one typed value in Rust and
one closed schema arm on the wire. A kill names its target. A
`step-limit-reached` record names the target and carries a nonzero allowance
plus exactly its first excluded count. `waited`, `unconfirmed`, and `errored`
name the target on which no verdict was established. Outcomes that did not
occur on a target name none. A boundary on any other outcome, or a step-limit
outcome without one, is not a v2 document.

`step-limit-reached` is an execution fact, not a detection. It is a hole in
the verification, contributes to neither side of a score, is never cached or
checkpointed, and cannot be answered by an acceptance. Historical v1
`runaway` records have no matched control and are never reinterpreted as v2.

Every mutation row also carries an explicit `accepted` boolean. This makes
the durable `accounting.mutants.accepted` column independently derivable;
`true` is valid only for `survived`, `unreached`, or `equivalent` rows.

`blind_in` names each build in which a mutation remains a hole, using exactly
`unnoticed`, `unreached`, `step-limit-reached`, `waited`, or `errored`.
Answered builds cannot be represented in that field.

## Findings

A **finding** is an actionable defect or an explicit gap in what the run
established. There are twelve kinds, and every report carries the stable name:

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

The last column is derived from the same closed `FindingKind` that decides the
verdict. A report with a defect concludes `DEFECT`; a report with only gaps
concludes `INSUFFICIENT`; an assurance carries no findings.

## Who decided each mutation

`accounting.mutants.observers` partitions the catalog. There are ten columns,
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

The accounting is re-derived exactly from the ID-level records. Mutation and
target identities are unique, target rows are in canonical order, every
target status column is reproduced from the rows, and every mutation outcome,
reuse, and acceptance column equals the row-derived count. In particular:

```text
cataloged = rejected + executed + unreached + equivalent
executed >= killed + survived + step_limit_reached + waited
accepted <= survived + unreached + equivalent
reused_killed <= killed
reused_survived <= survived
observers.total() = cataloged
```

Only killed and survived mutation evidence is reusable. Model answers retain
their generated source, raw export, process termination, hashes, pinned tool
and backend identity so an independent audit can re-derive the affirmative
answer rather than trusting a summary.

Every model identity also carries one closed `crate_input` object. Its package
is `njutest-verified-model`, edition is `2024`, source is `src/lib.rs`, network
policy is offline, and `dependency_resolution` is exactly
`empty-lock-offline-v1`: the dependency-free manifest, exact empty lockfile,
offline execution, and pre/post whole-crate byte check replace a Cargo flag
which Kani 0.68 does not accept. Its `environment` is
`minimal-v1`: the verifier receives a newly constructed environment containing
only the fixed Cargo and Kani homes, checker path, private target/temp paths,
explicit target, offline mode, and empty compiler-wrapper/flag slots. Caller
Python, toolchain, compiler, wrapper, Nix, and dynamic-loader variables are not
inherited. Its SHA-256 is a domain-separated digest over that policy, the exact
fixed `Cargo.toml`, fixed `Cargo.lock`, and retained generated source. Rust
deserialization checks that digest against
the rendered-source digest; `modelaudit` independently reads the source and
recomputes the same crate input from its own constants. Kani therefore cannot
earn an affirmative row from an unreported subject manifest, dependency,
build script, target, or stale lock resolution.

This evidence pins and rechecks observable verifier identities; it is not a
software-supply-chain attestation for the host. A local actor able to replace
the checked `cargo-kani`, Cargo/rustup proxy, Kani bundle, operating system, or
files between validation and execution remains inside the documented machine
trust boundary.

## Model records

Every `models[]` entry has exactly two outer fields: the full `mutant`
identity and a nested `answer`. The nested value is one closed arm:
`ineligible { decision, reason }`, `undecided { decision, attempt: { reason,
evidence } }`, or affirmative `noticed`/`proved { decision, evidence }`. Keeping the answer
behind an explicit field prevents a decision arm from merging its namespace
with the record identity, and both the schema and `proofaudit` reject unknown
fields at either level.

Under `verified-v1`, every `survived`, `model-noticed`, and `model-proved`
mutation row has exactly one model record. Conversely, every model record
names exactly one mutation row, and its answer agrees with that row;
duplicates and missing counterparts are rejected. `standard-v1` and `deep-v1`
do not run this model phase, so both retained model records and model-decided
mutation outcomes are invalid under those contracts.

## Routing

`mutants[].routing` carries the premise of every target-level conclusion:

| field | what it says |
| --- | --- |
| `granularity` | `all`, `block`, `test`, `discharged`, or `unreached` |
| `reaching` | targets that could notice the mutation |
| `discharged` | targets removed by `branch-never-taken` or `never-infected` |
| `fallback` | why routing widened: `not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, or `touch-incomplete` |
| `answered` | targets actually asked, in order, with their outcomes |

Evidence consultation records either the source run it reused or one closed
refusal: `nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`,
`key-changed`, `not-passing`, `target-entered`, or `nothing-routed`.

## Shards and projections

A `K/N` shard owns dense catalog indices whose index modulo `N` is `K - 1`.
A part concludes `PARTIAL`; only a complete, non-overlapping set of all parts
can be merged into an unsharded verdict. The merge re-derives accounting,
findings and verdict from the union instead of adding claims from the parts.

JSON is canonical. Terminal output is tab-separated with the record kind
first and verdict last; untrusted text is escaped. HTML, SARIF and JUnit carry
the same audit identity and findings.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `PARTIAL`, `RESOLVED` |
| 1 | `DEFECT`, `REPRODUCED` |
| 2 | `INSUFFICIENT`, `INCONCLUSIVE` |
| 3 | `ERROR`, invalid input, or an infrastructure failure |
| 130 | interrupted |
| 143 | terminated |

`njutest --help` renders the same table from the verdict types; the ledger
test holds this page to that output.
