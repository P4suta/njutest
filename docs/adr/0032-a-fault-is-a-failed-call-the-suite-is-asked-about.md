<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0032 — A fault is a failed call the suite is asked about

## Status

Accepted, 2026-10-01.
The dimension `faulted-v1` of the assurance plan: what a run establishes when a call the program makes fails.
Implemented by `inject-error`, the runtime's `Injectable` and its record of where a failure went, `njutest::assure::faults` and the proofaudit `faults` layer.
The table names the implementation and independent audit that hold each decision.

| Decision | Held by |
| --- | --- |
| 1, a fault site is a `?` in a measured file | the fault record's rule, version, byte span, original, replacement and whole-file source digest; the proofaudit identity layer and engine audit re-mint each fault; the committed `engine-run-faulted` recording holds six accepted and two rejected sites; `syntax::a_fault_is_asked_only_where_a_runtime_can_be_called` holds const contexts and macro expansions to the walker's rules; `proofaudit::a_fault_record_whose_fields_do_not_mint_its_identity_is_refused` plants a mismatched identity |
| 2, a fault replaces the call with its failure | the six `Injectable` implementations of the generated runtime; `execute::tests::every_error_a_fault_can_make_is_made_and_there_are_exactly_six`, which makes each of them and pins that there are six, in the native and the sealed module; the refused sites of `fixture-faulted` |
| 3, activated like a mutant, and beside one | `Perturbing`, which has no `PartialEq`, so every stage `run_resuming` shares routes on it by an exhaustive match and a faulted session compares no reach and repairs nothing; `toolchain_faults::a_faulted_session_compares_no_reach_and_runs_nothing_again` |
| 4, only reach decides where a fault is asked | `Perturbing` keeps faults outside the mutation discharge and repair paths; each fault route's reaching is held to every target in the recorded faulted baseline, with unknown targets refused; `toolchain_faults::the_faulted_baseline_s_reach_is_recorded_so_every_route_s_reaching_holds_to_it` and the proofaudit planted mismatch; the unreached site of `fixture-faulted` |
| 5, a fault has its own decisions | `FaultDecision` and `njutest::assure::faults`; the proofaudit `faults` layer holds each decision to its executions, every reaching target, attribution records and the recorded before/after write sets; `toolchain_faults::a_run_asked_for_faults_says_which_failed_calls_the_suite_noticed` audits eight sites, including unreached, waited and all-declined outcomes, and the absorbed call reached by two targets; the write and failure-write fixtures and planted unsaid write hold attribution |
| 6, a survivor asked again under the fault is not a kill | transitive guard propagation, held by `instrument::a_fault_guard_is_carried_into_every_alternative_that_keeps_its_bytes`; the `beside` records and proofaudit's re-derivation of both `failed: alone` and `failed: beside`; the ignore-question toolchain run, audited against its source and recording; `report_lines::every_drawing_states_the_evidence_a_fault_gave_about_a_survivor` and the lines, HTML, SARIF and JUnit goldens; the runtime's stop on an unknown fault, which `Observation::refused` reads as never a kill |

Every fault toolchain run that asserts audited evidence gives proofaudit its report, trace and source root.
The extended fixture and the ignore-question fixture hold the end-to-end decisions and drawings to those records.

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
   This propagation follows retained call bytes through enclosing rewrites, including `ignore-question-statement`, which rewrites the whole statement.
   An alternative that removes the call carries no fault guard and is not asked beside that fault.
   The faulted session holds the error-propagation mutations beside the faults for that and judges only the faults; each survivor is put beside its fault, target by target in name order, against the fault alone on the same target, and every pair of runs is recorded.
   The first target on which two pairs agree that exactly one run failed is the part's `beside` record — the `observable-under-fault` evidence — and the audit re-derives it from the recorded pairs.

## Consequences

- The two accepted survivors of `prove.rs` propagate `EngineError`, which the engine does not make, so their faults are `not-put` even though point 6 supports both error-propagation rewrites.
  Both stay in `.rust-mutants.toml` with their reasons until a test fails those calls; removing them would still be the unsound claim of point 6.
- Every `?` reached by a test costs one more execution, every surviving error-propagation mutant one more, and every fault every reaching test passed one more on each target until one does not bear out that it was absorbed; a site the compiler refuses costs a validation round and nothing after it.
- A user's own error type is never injected.
  Implementing a trait of ours in their tree would put our code in their program's type system, and guessing a constructor would inject something their code never returns; both are refused by construction, and the site says `not-put`.
