<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0032 — A fault is a failed call the suite is asked about

## Status

Proposed, 2026-09-24.
The dimension `faulted-v1` of the assurance plan: what a run establishes when a call the program makes fails.
Implemented by `inject-error`, the runtime's `Injectable` and its record of where a failure went, `njutest::assure::faults` and the proofaudit `faults` layer; `absorbed` landed on 2026-09-29.
The table says what holds each decision so far, and the list after it what still stands between it and acceptance, as a reading of the tree on 2026-09-29 found it.

| Decision | Held by |
| --- | --- |
| 1, a fault site is a `?` in a measured file | not held |
| 2, a fault replaces the call with its failure | the six `Injectable` implementations of the generated runtime; `execute::tests::every_error_a_fault_can_make_is_made_and_there_are_exactly_six`, which makes each of them and pins that there are six, in the native and the sealed module; the refused sites of `fixture-faulted` |
| 3, activated like a mutant, and beside one | `Perturbing`, which has no `PartialEq`, so every stage `run_resuming` shares routes on it by an exhaustive match and a faulted session compares no reach and repairs nothing; `toolchain_faults::a_faulted_session_compares_no_reach_and_runs_nothing_again` |
| 4, only reach decides where a fault is asked | not re-examined by the reading of 2026-09-29: `record_fault_route` records each route, and the proofaudit `faults` layer holds `unreached` to a route that reached nothing, but nothing checks that no discharge was applied to one |
| 5, a fault has its own decisions | `FaultDecision` and `njutest::assure::faults`; the proofaudit `faults` layer, which reads a decision from the executions that asked the suite, holds `unnoticed` and `absorbed` to every reaching target, `noticed` to the first target in name order that noticed, and `fault-write-unattributed` to the attribution records; `faults::a_fault_nobody_noticed_was_put_to_every_target_that_reaches_it_and_only_its_own_runs_count`, `faults::a_fault_is_noticed_by_the_first_target_in_name_order_that_notices_it`, and the planted unsaid write of the faults sentinel |
| 6, a survivor asked again under the fault is not a kill | the `beside` records and the proofaudit's re-derivation of them; the `why` step `observable-under-fault`, held by `toolchain_faults::why_names_a_survivor_the_suite_tells_apart_under_a_fault_observable_under_fault`; the runtime's stop on an unknown fault, which `Observation::refused` reads as never a kill, held by `execute::tests::a_process_its_runtime_ended_for_the_apparatus_is_never_a_kill` |

- Decision 1: the engine audit re-mints an `inject-error` row only of `rust-mutants run --operator inject-error`, and no committed engine run holds one; a fault njutest reports carries no rule, span or digest to re-mint from.
  A `?` in a const context and one a macro expands to are held by the walker's general rules, with no fault test.
- Decision 5: the paths written before the first fault and after the last are not recorded, so the audit holds the unattributed finding only to the paths attribution was asked about.
- Decision 6: the evidence is drawn on the `why` page and in the JSON, and in no report drawing; `ignore-question-statement` carries nothing only because njutest selects the whole family.
- End to end, nothing runs an unreached `?`, a waited or declined fault, a fault several targets reach, or `failed: alone`.

## Context

Mutation perturbs the handler and never the callee.
`question-to-unwrap` turns `expr?` into `expr.unwrap()`, and `ignore-question-statement` turns `expr?;` into `expr;`; while `expr` succeeds, the original and both mutants run identically, so a suite that only takes the happy path cannot notice them.
This repository's own ledger shows it: two of its accepted survivors, both in `rust-mutants/src/prove.rs`, say in their reason that they are "a fail-open the suite cannot reach rather than an equivalence".
Nothing in the run ever makes a call fail.

A wire-level fault ([ADR 0021](0021-a-claim-is-a-perturbation-an-observer-and-a-decision.md)'s seam dimension) fails a socket the program talks through.
This decision fails a call inside the program, at the place it asks whether the call succeeded: the `?`.

## Decision

1. **A fault site is a `?` in a measured file.** Discovery names each `expr?` once as the rule `inject-error` of the family `fault`, which no tier chooses: a run asks for faults by name, and a tree's catalog, fates and verdict under any tier are unchanged by them.
   The rule's name is part of every identity digest, so no fault can share an identity with a mutant, and the engine audit's identity layer re-mints it like any other.
   A `?` in a `const` context is never a site, since nothing there can call the runtime; one a macro expands to is invisible, like every mutant would be.

2. **A fault replaces the call with its failure.** Under a fault, `expr?` evaluates `Err(injected)` instead of `expr`: the callee fails before it does anything, which is the failure every caller has to be ready for.
   It is written the way every mutant is, as a branch of the guard at the site — `if active(i) { Err(injected()) } else { expr }` — so the two branches unify, the error type comes from `expr`, `expr` is not evaluated under the fault, and the borrow checker sees the original code.
   `injected` is built by a trait the generated runtime declares, `Injectable`, implemented for exactly the error types the engine can make without guessing: `std::io::Error` (`ErrorKind::Other`, carrying the runtime's own payload, so `unnoticed` reads as unnoticed as `Other`), `std::str::Utf8Error`, `std::string::FromUtf8Error`, `std::num::ParseIntError`, `std::num::ParseFloatError` and `std::num::TryFromIntError`.
   A site whose error type is anything else — a user's own error type, an `Option` — does not compile under the fault, and the validation rounds that already condemn a mutant the compiler refuses condemn it with the compiler's words: the fault is `not-put`, which is a statement about the engine and never a finding about the program.

3. **A fault is activated like a mutant, and beside one.** On its own it is activated through `RUST_MUTANTS_ACTIVE`, like any cataloged perturbation; for the composite of point 6, `RUST_MUTANTS_FAULT` names a fault active beside the mutant.
   `RUST_MUTANTS_FAULT` is composed and reserved exactly as `RUST_MUTANTS_ACTIVE` is: stripped from every baseline and control, never nameable by a knob, and a faulted run's touch and drift records are never compared with the baseline's.

4. **Only reach decides where a fault is asked.** The two programs are identical up to the site, so a test that never reached it runs the same under the fault, and `unreached` is sound.
   Reach is measured at the `?` itself, by the guard the fault already has there, not at the body around it.
   Nothing past the site is: `never-infected` and `branch-never-taken` rest on a measurement of the program without the fault, whose control flow the fault changes, so no discharge is applied to a fault's route ([ADR 0004](0004-proof-layers-not-budgets.md) decision 2: a layer that cannot establish its premise keeps the execution).

5. **A fault has its own decisions**, `FaultDecision`, and none are added to the shared `Decision` ([ADR 0022](0022-composition-needs-two-layers-answering-one-question.md)): `noticed` (a test failed under the fault, passed on the unchanged program, and failed under it again; `by` is the first such target in target order, where the judging stops), `unnoticed` (every test that reached the site passed under it), `absorbed` (every test that reached the site passed under it, and the failure went nowhere anything read it, below), `unreached`, `waited` (a bound expired before a test finished, which a caller retrying until the call succeeds legitimately does under a failure that never stops), `undecided` (a failure that did not reproduce, or a test that could not be run), and `not-put`.
   Writing into the tree under measurement is not one of them.
   The faulted executions share one copy of the tree and run in parallel, so the run notes which paths of the tree already differ from what was instrumented before the first fault is put and again after the last, and ties each path first written in between to one fault before concluding anything: the file is removed, one fault is run alone on one target that reached it, and where the path is written again while that target passes, the file is removed once more and the target runs alone without the fault.
   A path the fault wrote and the unfaulted run did not is `broken-under-fault`, a `DEFECT`, since that is the program doing something with a failed call that reaches past where it was asked to work; every other path is a `not-measured` finding about `fault-write-unattributed`.
   The faulted run has to pass: a test that noticed a fault by failing can write the tree because of its own failure — `proptest-regressions`, `*.snap.new`, a file named after its process — and a verdict drawn from that would be one the run produced by how it measured ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md)).
   The comparison is by path: a file every run writes, written again differently under a fault, is not caught, because telling a fault's rewrite from the next ordinary run's would take a digest that a file with a timestamp in it never keeps.
   A panic is not a defect: plenty of programs stop on a failed write and say so, and calling that a defect is a product nobody uses.
   Whether a caller saw the failure at all is `absorbed`, and the runtime keeps the record it rests on: the `std::io::Error` a fault makes carries a payload of the runtime's own, which appends `made` when it is made, `read` whenever something formats it, and `dropped` when it is dropped, to a file `RUST_MUTANTS_FAULT_FATE` names outside the scratch, and a line it cannot write stops the process.
   A fault every reaching test passed is run again on each of those targets in name order with that record asked for, stopping at the first run that does not bear it out, and each run is recorded as a `fault-fate` with its outcome and counts.
   It is `absorbed` where every such run passed and dropped every failure it made without anything reading it: the program went on as if the call had not failed, and nothing a person or a test could read carried the failure.
   Anything else stays `unnoticed`: a failure something formatted, a run that failed, a failure still held when the process ended, and every error type but `std::io::Error`, which has nowhere to carry the record.
   A caller that wraps the error in one of its own and shows only its own words has absorbed it by this measure, which the limitations page says.
   An `unnoticed` or `absorbed` fault is an `unnoticed-fault` finding and makes the run `INSUFFICIENT`, not `DEFECT`: it changes what the program is given rather than the program, and shows that no test asserts on that failure path.

6. **A survivor is asked again under the fault at its own site, and what that shows is not a kill.** An error-propagation mutant — `question-to-unwrap`, `ignore-question-statement` — that survived every run is put again with the fault at the same `?` active beside it.
   If the suite then tells the mutant from the original, the mutant is `observable-under-fault`: evidence that it is not an equivalence, attached to the survivor, in no kill count and no score.
   It is not a kill, because no test made the call fail; the engine did, and counting it would have the report claim a failure path is covered that no test exercises — a verdict produced by how the run measured ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md)).
   The survivor stays a gap, and the fault dimension says what closes it: the site whose failure the suite would notice, and that no test makes fail.
   The composite rests on the instrumentation: a fault's guard whose every alternative is a fault is carried into the alternative of every mutation of the site it is a child of that keeps the call's bytes, so `RUST_MUTANTS_FAULT` can make it active inside that branch.
   The instrumenter records each pair it carried, the session reads the pair back rather than deriving it again, a request for any other pair is refused, and the runtime stops a process named an unknown fault rather than running the mutation alone.
   A rewrite above the call's node — `ignore-question-statement` rewrites the whole statement — carries nothing, and its survivors are not asked.
   The faulted session holds the error-propagation mutations beside the faults for that and judges only the faults; each survivor is put beside its fault, target by target in name order, against the fault alone on the same target, and every pair of runs is recorded.
   The first target on which two pairs agree that exactly one run failed is the part's `beside` record — the `observable-under-fault` evidence — and the audit re-derives it from the recorded pairs.

## Consequences

- Neither accepted survivor of `prove.rs` can gain that evidence: one is an `ignore-question-statement`, which point 6 does not ask, and the other propagates `EngineError`, which the engine does not make, so its fault is `not-put`.
  Both stay in `.rust-mutants.toml` with their reasons until a test fails those calls; removing them would still be the unsound claim of point 6.
- Every `?` reached by a test costs one more execution, every surviving error-propagation mutant one more, and every fault every reaching test passed one more on each target until one does not bear out that it was absorbed; a site the compiler refuses costs a validation round and nothing after it.
- A user's own error type is never injected.
  Implementing a trait of ours in their tree would put our code in their program's type system, and guessing a constructor would inject something their code never returns; both are refused by construction, and the site says `not-put`.
