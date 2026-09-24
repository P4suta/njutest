<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0032 — A fault is a failed call the suite is asked about

## Status

Proposed, 2026-09-24.
The dimension `faulted-v1` of the assurance plan: what a run establishes when a call the program makes fails.

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
   `injected` is built by a trait the generated runtime declares, `Injectable`, implemented for exactly the error types the engine can make without guessing: `std::io::Error` (`ErrorKind::Other`, which the fault's record names, so `unnoticed` reads as unnoticed as `Other`), `std::str::Utf8Error`, `std::string::FromUtf8Error`, `std::num::ParseIntError`, `std::num::ParseFloatError` and `std::num::TryFromIntError`.
   A site whose error type is anything else — a user's own error type, an `Option` — does not compile under the fault, and the validation rounds that already condemn a mutant the compiler refuses condemn it with the compiler's words: the fault is `not-put`, which is a statement about the engine and never a finding about the program.

3. **A fault is activated like a mutant, and beside one.** On its own it is activated through `RUST_MUTANTS_ACTIVE`, like any cataloged perturbation; for the composite of point 6, `RUST_MUTANTS_FAULT` names a fault active beside the mutant.
   `RUST_MUTANTS_FAULT` is composed and reserved exactly as `RUST_MUTANTS_ACTIVE` is: stripped from every baseline and control, never nameable by a knob, and a faulted run's touch and drift records are never compared with the baseline's.

4. **Only reach decides where a fault is asked.** The two programs are identical up to the site, so a test that never reached it runs the same under the fault, and `unreached` is sound.
   Reach is measured at the `?` itself, by the guard the fault already has there, not at the body around it.
   Nothing past the site is: `never-infected` and `branch-never-taken` rest on a measurement of the program without the fault, whose control flow the fault changes, so no discharge is applied to a fault's route ([ADR 0004](0004-proof-layers-not-budgets.md) decision 2: a layer that cannot establish its premise keeps the execution).

5. **A fault has its own decisions**, `FaultDecision`, and none are added to the shared `Decision` ([ADR 0022](0022-composition-needs-two-layers-answering-one-question.md)): `noticed` (a test failed under the fault, passed on the unchanged program, and failed under it again; `by` is the first such target in target order, where the judging stops), `unnoticed` (every test that reached the site passed under it), `unreached`, `waited` (a bound expired before a test finished, which a caller retrying until the call succeeds legitimately does under a failure that never stops), `undecided` (a failure that did not reproduce, or a test that could not be run), and `not-put`.
   Writing into the tree under measurement is not one of them.
   The faulted executions share one copy of the tree and run in parallel, so a write cannot be attributed to one site; the run notes which paths of the tree already differ from what was instrumented before the first fault is put and again after the last, and a path first written in between is the phase's `broken-under-fault` finding, a `DEFECT`, since that is the program doing something with a failed call that reaches past where it was asked to work.
   The comparison is by path: a file every run writes, written again differently under a fault, is not caught, because telling a fault's rewrite from the next ordinary run's would take a digest that a file with a timestamp in it never keeps.
   A panic is not a defect: plenty of programs stop on a failed write and say so, and calling that a defect is a product nobody uses.
   Whether a caller saw the failure at all — the `absorbed` of the plan — needs a record of where the injected error went, which the runtime does not keep yet; until it does, an absorbed failure reads as `unnoticed`, which is what the suite could tell.
   An `unnoticed` fault is an `unnoticed-fault` finding and makes the run `INSUFFICIENT`, not `DEFECT`: it changes what the program is given rather than the program, and shows that no test asserts on that failure path.

6. **A survivor is asked again under the fault at its own site, and what that shows is not a kill.** An error-propagation mutant — `question-to-unwrap`, `ignore-question-statement` — that survived every run is put again with the fault at the same `?` active beside it.
   If the suite then tells the mutant from the original, the mutant is `observable-under-fault`: evidence that it is not an equivalence, attached to the survivor, in no kill count and no score.
   It is not a kill, because no test made the call fail; the engine did, and counting it would have the report claim a failure path is covered that no test exercises — a verdict produced by how the run measured ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md)).
   The survivor stays a gap, and the fault dimension says what closes it: the site whose failure the suite would notice, and that no test makes fail.
   The composite rests on the instrumentation: a fault's guard is carried into the alternative of every mutation at its site that keeps the call's bytes, so `RUST_MUTANTS_FAULT` can make it active inside that branch, and `Session::fault_beside` names exactly the fault so carried.
   The faulted session holds the error-propagation mutations beside the faults for that and judges only the faults; each survivor is put beside its fault, target by target in name order, against the fault alone on the same target, and the first target on which exactly one of the two failed — the one beside it confirmed by a second run — is the part's `beside` record.

## Consequences

- The two accepted survivors of `prove.rs` gain the evidence that they are not equivalences, and stay in `.rust-mutants.toml` until a test fails those calls; the plan's acceptance of removing them is replaced by that, because removing them would be the unsound claim of point 6.
- Every `?` reached by a test costs one more execution, and every surviving error-propagation mutant one more; a site the compiler refuses costs a validation round and nothing after it.
- A user's own error type is never injected.
  Implementing a trait of ours in their tree would put our code in their program's type system, and guessing a constructor would inject something their code never returns; both are refused by construction, and the site says `not-put`.
