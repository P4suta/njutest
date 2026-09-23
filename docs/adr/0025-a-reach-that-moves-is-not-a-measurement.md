<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0025 — A reach that moves is not a measurement

## Status

Accepted, 2026-09-23.
Implemented by `touch::unions_differ` and the recording `Session::control` makes when asked to, by the `unstable-baseline` finding and the `drift-not-measured` limitation of njutest, and by the `drift` layer of `xtask proofaudit`, which was written and planted before the rule.
Refines [ADR 0014](0014-the-guards-are-the-measurement.md) and applies [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md) to the measurement every proof layer stands on.

## Context

Every proof layer of [ADR 0004](0004-proof-layers-not-budgets.md) removes executions by reading one run: the baseline, on which each target's guards recorded which of its tests reached which site ([ADR 0014](0014-the-guards-are-the-measurement.md)).
A mutation no test reached is `unreached`; a target no test of which reached it is not run; a target whose guard never saw its branches part, or that never entered the body a branch proof names, is discharged.
Each of those reads `reaching(m, t)` off that one record.

That is sound only if what a target reaches is a function of the target.
A suite whose reach depends on anything else — the order libtest happened to schedule threads in, the clock, a hash seed, or state an earlier process of the same run left behind — gives a record that is one sample of a distribution, and every proof on it is a proof about that sample.
Nothing in the baseline can tell the two apart, because the baseline is one run.

A second run of every target already exists.
Before a kill is believed, the run confirms it with a pair, and the first half of the pair is an original-code control of the whole target (`assure::mutation::confirm`), with the same harness arguments, base environment, and working-directory rule as the baseline.
It ran with its guards silent.

## Decision

1. **A control records what it reached, and the run compares it with the baseline.** `Session::control` takes an explicit argument saying whether to record; the confirmation asks it to, and every other caller says no.
   The control appends to a log in its own execution's scratch, under the catalog's digest; a process that cannot record exits 96 as the baseline's does, and the control is run again without recording so that the kill is still confirmed or refused, and the target is `not-measured`.
   Both records are folded the same way, by `touch::attributed`, which moved out of the baseline's module so that there is one of it.

2. **What is compared is each target's union, not its tests.** Three unions per target — the sites anything of it reached (named threads and loose ones alike), the bodies it entered, and the sites it saw infected — are compared between the baseline and a control that passed exactly the tests the baseline passed.
   Per-test sets legitimately differ between two whole runs: which libtest thread first initialises a `OnceLock` decides which test a site is attributed to, and routing a test alone stays sound either way, because a test put on its own is established to answer the same question first ([ADR 0014](0014-the-guards-are-the-measurement.md)).
   A union cannot differ that way.
   Two whole-target runs over one tree that passed the same tests and reached different sets are a counterexample to "reach is a function of the target".
   A control that passed other tests is no comparison at all: its reach is the reach of other tests, and the target is `not-measured` rather than `held`.

3. **One observation suffices.** A counterexample is not strengthened by reproducing it, and a difference that did not reproduce on a third run would not make the first two agree.
   So the finding does not wait for a repetition, and nothing is a threshold ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md), "nothing here is a budget").

4. **The two runs are measured under equal conditions first.** A baseline ran with the run's shared scratch as its temporary directory, and a control with a fresh directory of its own.
   A suite that wrote into its temporary directory would then reach differently on the two for a reason that is about how the run measured, and a finding raised on it would be a finding about the apparatus — the class [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md) names.
   Every baseline process now gets a fresh execution-style scratch of its own, as every control and every mutant execution does, so a difference between them is a difference in the suite.
   A target's baseline keeps that one directory across the processes it may take — the retry of a target that did not pass, and the rerun of one that could not record — because a retry that recovers from what its first attempt left is the behaviour `baseline-passed-on-retry` already names, and a suite that recovers only that way has a control, fresh, that does not pass and confirms no kill on it.
   Nothing about a verdict moved when that changed: the fates of every fixture and the differential of `rust-mutants-cli` are the same before and after.

5. **A moved target is a finding, not a repair.** `unstable-baseline` names the target and counts the dispositions resting on the moved measurement that a proof decided outright: mutations a discharge on that target removed the execution of and that nothing killed, and `unreached` claims, each of which says that target reached nothing.
   A mutation every reaching target of which was discharged is `survived` in this report, not `proved`, which is kept for the compiler's equivalence proof; so the count is of survivors whose route discharged the target, not of a `proved` column.
   A kill is existential and rests on no discharge, so a killed mutation is not counted even where its route discharged the target.
   It is not a defect in the code under test, so the verdict is `INSUFFICIENT` rather than `DEFECT`.
   Re-executing those dispositions without the proofs that rested on the moved record is the repair, and it is not done here; it is the next change, and until it lands the finding is what a reader acts on.

6. **Absence is a case.** A target whose baseline was measured and that no comparable control recorded is `not-measured`, and the `drift-not-measured` limitation counts and names them.
   It is not `held`: a target nothing killed is never confirmed, so a run in which nothing was killed has compared nothing, and says so.

7. **Where it is recorded.** Each catalog part records one drift record per measured target — `held`, `moved` with what moved, or `not-measured` with why — and a checkpoint keeps what an interrupted run observed, so a resumed run does not lose a control it will not run again.
   A part that measured the whole catalog raises the finding and the limitation from its own records.
   A shard does not: the count is over the whole catalog and a target unmeasured in one part may be measured in another, so `njutest merge` raises both over the combined records, the fourth way out of [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md).
   A shard in which a target moved concludes `INSUFFICIENT` rather than `PARTIAL`, because the fact that it moved needs no other part.

8. **The audit came first.** The `drift` layer of `xtask proofaudit` re-derives, from the engine recording's `touch` records alone, which targets held, moved, or were not measured, and holds the report's drift records, findings, and limitation to it in both directions.
   It was written, and a defect planted for it, before the runner raised anything ([ADR 0022](0022-composition-needs-two-layers-answering-one-question.md) §3).
   Each `touch` record says which run it was measured on, `baseline` or `control`, rather than a new record kind carrying the same fields: the two are one measurement of one target on two runs, and a comparison is sound only where both sides carry exactly the same fields, which one type makes unrepresentable to get wrong.

## Consequences

- A suite whose reach depends on process-wide state an earlier process left, on time, or on order is told so, by target, the first time a kill confirms on it.
  `fixtures/fixture-drifts` is one: its test reaches one function when it is the first process of a run to look in the run's scratch and another when it is not.
- The control costs nothing it did not already cost: it ran before, and only its guards are now asked.
  Where a platform cannot record, the rerun without recording is one more process for that target, once.
- A remembered baseline ([ADR 0014](0014-the-guards-are-the-measurement.md), `measurements`) is read back without running anything, so it could carry a record that moved on the run that wrote it into every run after.
  njutest never recalls one, so every comparison it makes is against a baseline that ran in the same run.
  The engine compares against whatever baseline its session holds; a caller that recalls one and asks a control to observe compares against a record nobody watched this run.
  Before that comparison could mean what this one does, a remembered baseline would have to carry the standing the run that wrote it established, and be refused as an answer where that standing was not `held`.
- Writing the audit first found that `xtask proofaudit` had not been reading real runs at all.
  A complete report holds its facts per build and per part, the audit read a flat document, and every run of `njutest verify` was refused with exit code 2; flattened by hand, the one real run tried drew nine violations, every one of them a runner `mutant-exec` naming its target by digest where the route named it by name.
  The audit now projects a report of one build measured whole onto the view it re-decides, and the execution names its target as the route does, so the layer this ADR adds is held to a real recording rather than only to its planted specimen.
- The comparison sees only what the guards see.
  A suite whose behaviour depends on the environment in a way no guard records — a branch with no mutant inside it — moves without this noticing, the blind spot every layer of [ADR 0004](0004-proof-layers-not-budgets.md) shares.

## Alternatives

- **Compare per test.** It would raise a finding on every suite that initialises shared state lazily, which is most of them, about a difference routing is already sound under.
- **Run the baseline twice.** It doubles the most expensive fixed cost a run has to measure what a control measures for free, and it would still be one comparison per target.
- **Wait for the difference to reproduce.** A counterexample does not need a second witness, and a policy of waiting is a threshold under another name.
