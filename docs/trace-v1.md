<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Trace v1

Published schema: [`schema/njutest-trace-v1.json`](../schema/njutest-trace-v1.json).

**Status: implemented.** Current runner recordings identify themselves as `njutest-trace-v1`.
Historical v1 recordings remain documented separately and are never claimed to have this shape.
A trace is diagnostic exhaust, never evidence, and cannot establish a verdict or cache identity.
An explicitly requested trace is nevertheless a command requirement: directory setup fails before verification starts, and a durable write or final-sync loss fails the command rather than being hidden by the in-memory diagnostic ring.

Every JSON Lines event is the closed envelope `seq`, `timestamp`,
`elapsed_ms`, and `payload`.
`payload` is a closed tagged object whose `type` selects one record.
Nesting makes an envelope/payload field collision unrepresentable.
The stream begins with `run-start` and ends with exactly one `run-end`.
Sequence gaps, a missing end, and dropped events are reported.

`Fields` below is the exact serialized key set, including optional keys.
A test serializes closed specimens and compares this table in both directions.

| Type | Fields | Records |
| --- | --- | --- |
| `run-start` | `schema`, `njutest`, `rust_mutants`, `run_id`, `run_kind`, `contract` | current schema and run identity |
| `phase-start` | `name`, `duration_ms` | one phase beginning |
| `phase-end` | `name`, `duration_ms` | the matching phase end and duration |
| `exec` | `argv`, `dir`, `env_names`, `timeout_ms`, `stopped`, `duration_ms`, `output_bytes`, `output_sha256`, `output_truncated`, `output_path`, `error` | one command and its single tagged termination; environment names, never values |
| `progress` | `message`, `subject`, `done`, `total` | bounded progress through a phase |
| `artifact` | `kind`, `path`, `bytes` | a retained file or directory |
| `route` | `mutant`, `granularity`, `fallback`, `reaching`, `tests`, `discharged`, `considered`, `reused`, `refused`, `rule`, `carry_refused` | one mutation route: `all`, `block`, `test`, `discharged`, or `unreached`; fallback `not-measured`, `position-unknown`, `outside-blocks`, `coverage-incomplete`, or `touch-incomplete`; reuse refusal `nothing-recorded`, `unreadable`, `target-unknown`, `not-routed`, `key-changed`, `not-passing`, `target-entered`, `nothing-routed`, `superseded`, which a native lead a store kept gets where the run's sealed executions decided the mutation (ADR 0046), or `unreproduced`, which a sealed verdict a store or a checkpoint kept gets where the run's sealed executions of the mutation did not come to what it recorded, in the order they ran, or did not establish the verdict it records, and which an `unreproduced` note explains; the store a reused answer came out of, `exact` for this tree's own or `carried` for one an earlier tree left across an edit no execution of it entered ([ADR 0041](adr/0041-an-answer-carries-across-an-edit-it-never-entered.md)); and where a carried answer was found and not believed, the premise it failed: `skeleton-changed`, `item-changed`, `unsealed`, `entry-incomplete`, `route-grew`, `filter-differs`, `reach-moved`, or `uncontrolled` |
| `mutant-exec` | `mutant`, `target`, `args`, `outcome`, `step_boundary`, `duration_ms`, `alone` | one target execution; the checked boundary exists only for `step_limit_reached` |
| `sealed-exec` | `mutant`, `target`, `test`, `came_to` | one sealed execution a mutation's verdict rests on: the test, alone in its own instance on the target's sealed module, and what it came to, as the report's `evidence` spells it ([ADR 0046](adr/0046-a-verdict-is-what-a-sealed-run-observed.md)); under the `rerun` phase, one a stored report rests on, run again before the report is reissued, and one that could not be made again is a `rerun-unmade` note instead; outside it, one a sealed verdict a store kept rests on, run again on this run's bench before the verdict is believed ([sealed execution](engine/sealed.md#reproducing-a-sealed-verdict)) |
| `fault-exec` | `fault`, `role`, `target`, `args`, `outcome`, `duration_ms`, `alone` | one target execution with a fault failing a call, `first`, the `confirmation` after a failure, or an `attribution` or `attribution-control` run alone; no reader of mutant executions, routes, probes or drift reads one |
| `fault-fate` | `fault`, `target`, `outcome`, `fate` | one fault every reaching test passed, run again on one of them with its runtime recording what became of the failures it made: `fate` is `made`, `read` and `dropped` counted, or `null` where there was no record or it could not be read; `absorbed` rests on every reaching target's run passing with every failure made dropped and none read |
| `fault-writes` | `before`, `after` | the paths the tree had written before the first fault and after the last, which is what every unattributed write rests on, so the audit holds the unattributed finding to the paths the phase left written and not only to the ones attribution asked about |
| `fault-attribution` | `fault`, `target`, `path`, `faulted`, `passed`, `unfaulted` | one fault run alone on one target, whether it wrote a path the phase left in the tree and passed doing so, and what the target alone without it did (`not-asked`, `wrote`, `did-not-write`); `broken-under-fault` rests on `faulted` and `passed` with `did-not-write` |
| `fault-route` | `fault`, `reaching` | the targets whose baseline reached one fault's site, empty where nothing did, which is what `unreached` rests on |
| `fault-baseline` | `target`, `doc`, `reached` | what one target's faulted baseline reached, by the fault catalog's index, with whether a documentation target is one a route puts at every fault of its package whatever its own guards said, recorded before any fault is routed over it, so a route's `reaching` is held to the baseline and not to the route's own word |
| `fault-rejected` | `fault`, `diagnostic` | one fault the compiler refused, which is what `not-put` rests on |
| `fault-control` | `fault`, `target`, `passed` | whether the target passed on the original code when a fault's failure on it was confirmed, which `noticed` needs besides the failure repeating |
| `fault` | `catalog_index`, `id`, `display_id`, `path`, `rule`, `rule_version`, `span`, `source_digest`, `original`, `replacement`, `item`, `position`, `decision` | what one fault site came to, exactly as the report's `faults` holds it, with every field its identity is minted from |
| `beside` | `mutant`, `fault`, `target`, `failed` | a survivor a target told apart only with the call at its own site failing, exactly as the report's `beside` holds it |
| `beside-run` | `mutant`, `fault`, `target`, `alone`, `with` | one pair of runs of a target behind evidence beside a fault: the fault alone, then the survivor beside it; every pair is recorded, so the evidence is re-derived from these and not read back from itself |
| `crash-exec` | `crash`, `target`, `test`, `stage`, `sealed`, `exit_code`, `outcome`, `noticed`, `issued`, `left`, `unnamed`, `failed` | one run of a test a crash was put to, a sealed instance where `sealed` says so and a process otherwise: a process carries its `exit_code` and a native `outcome`, and a sealed instance carries a `null` `exit_code` and an `outcome` that is `halted`, where the host stopped it where its runtime publishes the notice, or what it came to judged against the test's control, as a sealed execution's record names it; `crash`, stopped at the call where `noticed` says the runtime published the notice of that stop, and `issued` is what the engine issued that run — the full `mutant`, the `catalog`, the `nonce` — and the notice it `read`, from which an audit decides the stop again, with the files and directories it `left`, or, where it left an entry whose name is not text, that entry spelled without loss as `unnamed` and nothing in `left`, which leaves the crash `undecided`; `next`, over what a stop left, a sealed instance started from what the halted one left, which is `unstartable` where what it left is no state an instance starts in, deciding nothing either way; `fresh`, in a scratch of its own, which a sealed crash never runs, since one sealed round decides it; with the tests either `failed`; every crash's decision is re-derived from these and its `crash-step` records in order |
| `crash-step` | `crash`, `taken` | one thing a run did about a crash besides running a test, `taken` by `kind`: `route`, the targets it `asked` in order, each with the `tests` that reach the call or `null` where which of them does is not known; `rejected`, the compiler refused it; `tainted`, an earlier stop wrote into the tree so it was not run; `outside`, a stop of it wrote into the tree; a crash's steps and runs together are every step its decision rests on, and the audit refuses a sequence the runner does not make |
| `crash` | `catalog_index`, `id`, `display_id`, `path`, `item`, `position`, `decision`, `sealed` | what one call that writes came to, exactly as the report's `crashes` holds it |
| `probe-exec` | `target`, `outcome`, `infected` | one infection measurement |
| `wire-exchange` | `capability`, `seq`, `during`, `duration_ms`, `read`, `request_bytes`, `response_bytes` | one raw or HTTP exchange; `read` is the closed `wire`/`method`/`path`/`status` object |
| `wire-exec` | `fault`, `capability`, `seq`, `rule`, `answer` | one licensed seam fault and its nested closed decision |
| `sentinel` | `layer`, `mutant`, `expected`, `routed`, `sighted` | one mutant planted for a layer that removes work before the baseline: `layer` is `reach`, `branch-never-taken`, or `never-infected:` followed by the form of evidence (`is-default`, `is-ok-default`, `is-some-default`, `is-true`, `inert-comparison`), `coverage` for the route the coverage measurement alone decides, or `equivalence`; `expected` is `unreached`, the proof that must discharge it, `kept-for-library` / `kept-for-tests`, or, for `equivalence`, `identical` / `rendered`; `routed` is what the engine decided, as a reader is told; a run ends in `ERROR` (`NJ5009`) after the first one not `sighted` |
| `model` | `mutant`, `answer` | one retained closed model question; `answer` is a nested closed decision and affirmative arms carry their full typed evidence |
| `control` | `target`, `test`, `asked_for`, `answer` | what the original code answered to one test asked to confirm a kill or a wait — `passed`, or `failed` with what it said — recorded the one time it ran, with the mutation whose asking ran it; every other asker of the same test is answered from it |
| `confirm` | `mutant`, `target`, `test`, `expected`, `answered_for`, `reproduced` | how one kill or wait was confirmed: the mutation whose control answered for the test, and what the mutation came to when run a second time — one of the engine's outcome names — or `null` where the control failed and it was not; a kill stands only where the control passed and the second run was a kill again, which `xtask proofaudit` re-derives |
| `resumed` | `mutant`, `killed_by` | one kill an interrupted run established and this run inherited from its checkpoint, once the sealed executions it rests on came out the same again on this run's bench, each a `sealed-exec` before it, where one that did not is an `unreproduced` note instead and the mutation is established afresh; it was confirmed in that run's recording, so `xtask proofaudit` leaves its confirmation unaudited here rather than calling it missing |
| `drift` | `mutant`, `observed` | what an original-code control established about one target's baseline reach: the one confirming `mutant`'s kill, or, where `mutant` is null, the one a target that confirmed no kill is run alone for; `observed` is the report's closed drift record ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)) |
| `repair` | `mutant`, `target`, `was`, `now`, `reached`, `by` | one disposition that rested on `target`, a target whose reach moved, run again against it: the outcome it `was`, the outcome it is `now`, whether that run `reached` the mutation's site (`reached`, `not-reached` or `unrecorded`), and what ran it `by`: `{ "kind": "native" }` for a lead run natively with its reach recorded, where a pass that did not reach the site leaves `now` equal to `was`, or `{ "kind": "sealed", "evidence": … }` for a sealed verdict put again on the sealed bench with the moved targets counted among those reaching it, where `evidence` is what that put established, in a report row's shape, and `reached` says whether it put a test of `target`; a sealed put that establishes nothing leaves the disposition a lead, `now` being what the native run then judged ([ADR 0036](adr/0036-what-rested-on-a-moved-reach-is-run-again.md)) |
| `knob` | `target`, `knob`, `standing` | what one control started with one knob put established about one target, the report's closed knob record: `stable`, `passed`, `broke` with the tests that failed, `moved` with the three unions, `uncompared`, `unsettled`, or `not-put` with why |
| `note` | `kind`, `detail` | a named diagnostic with no richer event type; `unreproduced` names the first execution that parted from a sealed verdict a store kept about one mutation when the mutation was put again, which is why the verdict was not believed: its place, target and test, what it was kept as and what it came to, or the verdict they establish where the one kept is another |
| `run-end` | `verdict`, `accounting`, `error`, `events_emitted`, `events_dropped` | the sole terminal event, emitted only after report persistence and cleanup succeed |

`run-end.accounting` is an explicit nullable field.
A complete run carries the final cross-build target and mutant counts plus `soundness_by_build`: one closed `{ build, accounting }` row for every configured build in request order.
Soundness is never selected from the first build or collapsed by a maximum.
A shard or a run that failed before it could complete carries `accounting: null`; absence of the key is malformed rather than another spelling of null.

`exec.stopped` is the engine's tagged termination union.
It cannot claim two terminal causes.
`mutant-exec.step_boundary` is the checked `limit` and exact `observed = limit + 1` pair from the nonce-correlated runtime notice; a bare exit code cannot manufacture it.

## Configured-build engine recordings

The runner recording owns a `builds/` directory whenever `--trace` is explicit.
Each requested build owns exactly one exclusive `builds/<zero-padded ordinal>/engine/` directory.
Names are report data, never path components.
Every engine `run-start.context` binds the final runner run id, its build-internal run id, ordinal, configured name, and canonical `BuildSelection` digest.
It binds the seven Cargo options selected by the configuration, while toolchain and resolved-host evidence remain separate.
A reader can therefore reject a missing namespace, a swapped trace, or a trace produced under different Cargo features, profile,
or target without trusting directory names or operator attention.
