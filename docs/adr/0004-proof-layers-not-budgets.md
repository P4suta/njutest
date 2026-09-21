<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0004 — Proof layers, not budgets

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0004 (accepted 2026-09-02).
Implemented by the coverage-region routing of `assure`, the branch-proof and never-infected discharges, the `route` events of the trace, and the `proofaudit` gate, as the milestones deliver them.
`rust-mutants` is a runner of the same kind and is held to the same rule: it measures its own coverage,
discharges with `prove::discharges` on it, records a `route` for every mutant it judges, and is re-decided by `cargo xtask engine-audit`.
This is the thesis of the project; it governs every speed-related decision.

## Context

A first verification of goatest's own repository ran for about three hours,
and 98 % of that was mutation: every mutant executed against every test that ran its file.
Every mutation-testing tool meets this wall, and the industry's answers are budgets — a time limit after which the remaining mutants are not run, a sample of the mutants, an exclusion list of slow packages, a model that predicts which mutants are worth running.

Each of those answers makes the tool faster by making its verdict weaker.
A mutant that was not run is a claim that was not tested, and a verdict that rests on untested claims is not the fail-closed verdict this project exists to give.
A budget is not an optimisation; it is a regression, however the setting is spelled.

A mutant has three ways to survive a test: the test never reaches the mutated code, or reaches it without infecting the state, or infects the state without the difference propagating to an assertion.
Each is a fact that evidence the run already collects can sometimes establish before any mutant is built.

## Decision

1. **No mutant is ever left unproved by policy.** There is no time budget,
   sampling rate, exclusion of slow targets, or prediction of which mutants to run, and none will be added.
   A run that is too slow is a run missing a proof, and the remedy is another proof.
2. **Every speed-up is a proof layer**: a rule that removes an execution because a lemma over evidence the run already holds says the execution could not observe the mutant.
   The layers are ordered by the way a mutant survives — reach (nothing of the target reached the site), infection (a branch proof, a guard that never saw its two branches part, or a probe that never saw the site differ), and propagation, which is the next to build.
   A layer that cannot establish its premise keeps the execution; the fallbacks are toward running more.
3. **The lemma and the premise live on different sides.** rust-mutants states what it can prove about a mutant from the source and the compiler — the branch proof, the probe form — and njutest checks the premise against its per-target evidence.
   Neither side trusts the other beyond the contract that names the claim.
4. **Every layer is visible.** A route records the granularity it was decided at, the fallback that widened it, and every target a proof discharged with the proof's name; a mutant resolved without an execution says so.
5. **Every layer is audited independently before it ships, and stays auditable.** `proofaudit` reimplements each rule from a recording and the coverage profiles a run left behind, and holds it to every kill that run proved: a layer that would drop one recorded killer is unsound, and a layer ships only with zero violations on a real recording of this repository.
   The code under audit is not asked whether it agrees with itself.

## Consequences

- The cost of a surviving mutant remains the bound on a run, and it comes down only as proofs come in.
  There is no setting that does it.
- A new proof is a change to both products, and this workspace holds both so that the change is one pull request: the engine gains a claim in its catalog, the runner gains a rule that consumes it, a trace vocabulary that shows it, documentation that states it, and an audit layer that checks it.
  A proof without all four is not finished.
- **And a fifth: the sentence a person reads when the layer works, held by a test of its own.** The four above establish that the proof is right; none of them establishes that a reader is told it happened.
  That is not a presentation concern.
  A survivor's sentence either says "write a test" or says "a proof removed this, so check the proof", and a reader who gets the wrong one acts on the wrong thing.
  Measured on both products, this is where the sentences went unheld: `string-to-empty` survived on the one sentence that names which proofs removed a mutation, leaving it to end mid-clause,
  and on fourteen field names inside a refusal, leaving it to say that something required was empty without saying what.
  A layer whose output is a sentence is not tested by asserting that it spoke, and a refusal is not tested by asserting that it refused: what a reader acts on is which thing it said was wrong.
- The layers share one blind spot: they see the coverage a test left behind.
  The limitations document names it once per layer.
- A reader who sees njutest go faster may ask which proof did it.
  The answer is always in the trace, never in a configuration file.
