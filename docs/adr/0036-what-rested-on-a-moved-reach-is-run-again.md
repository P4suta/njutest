<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0036 — What rested on a moved reach is run again

## Status

Proposed, 2026-09-24.
The repair [ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md) decision 5 left for the next change.
The table says what holds each decision so far, and the list after it what still stands between it and acceptance, as a reading of the tree on 2026-09-29 found it.

| Decision | Held by |
| --- | --- |
| 1, run again and replaced only where it reached | `repaired` and `repair` in `njutest::assure::mutation`, routed on `Perturbing` so a faulted session repairs nothing; `Session::exec_reaching`, which runs again with nothing to record a run that could not record, as a control is; `Observation::refused`, so the runtime's own stop is never a kill; `Route::reached_by`, from which the replaced disposition's route and routing are drawn; `left_for_later`, which files the replacement in the evidence and carry stores; `toolchain_verify::a_target_whose_reach_moved_has_every_disposition_resting_on_it_run_again` and `execute::tests::a_process_its_runtime_ended_for_the_apparatus_is_never_a_kill` |
| 2, the pair recorded and re-derived | the `repair` trace record; the proofaudit `repair` layer, whose `repair::derived` allows exactly what the aggregation can make of an outcome, which pairs a repair with its last touch, reads `was` as `judge` decides it, and whose `owed` holds every resting lead to a repair against each moved target and the repairs to their order; `proofaudit::a_repair_is_allowed_every_disposition_its_own_execution_can_come_to`, `a_repair_measured_again_alone_is_paired_with_the_touch_of_its_last_run`, `a_repair_touch_is_paired_with_the_repair_that_names_its_mutant_and_nothing_else`, `a_lead_resting_on_a_moved_target_that_nothing_ran_again_is_a_violation`, `repairs_out_of_the_order_the_repair_takes_are_a_violation` |
| 3, what still rests is a finding, and a limitation otherwise | `drift::found` and `drift::repaired`; `concluded_from`, the one function every verdict is drawn from, which reads a moved reach only for a part; the proofaudit `drift` layer's `held_to_findings` and `held_to_repairs`, and `held_to_counts`, which re-derives the counts both details give; `drift::a_moved_target_nothing_rests_on_is_concluded_as_the_published_verdict_concludes_it`, `proofaudit::an_unstable_baseline_finding_counts_what_the_rows_and_routes_leave_resting`, `proofaudit::a_reach_moved_limitation_counts_the_dispositions_the_repairs_replaced` |
| 4, a part repairs what it holds and a merge counts the rest | not held |

- Decision 1: only a lead is run again, so a sealed disposition resting on a moved target stays counted in `unstable-baseline`; and whether a disposition is asked depends on target names, since a hole one moved target leaves is never asked of the next.
- Decision 3: the `reach-moved` count counts every replacement, a hole among them, and its sentence calls each one decided again, where a hole is run again and decides nothing.
- Decision 4: a merge never states `reach-moved`, because how many dispositions a part ran again is held in the run (`Mutation::repaired`) and not in the part's record, so a merge cannot re-derive it; and nothing audits a merge's side of decisions 3 and 4.
- The checkpoint keeps only sealed kills and a repair replaces only a lead, so there is no checkpoint state for a repair to bring up to date.

## Context

A target whose control reached what its baseline did not is `moved`, and every disposition a proof decided from that baseline is unfounded: a survivor whose route left the target out, because a discharge or the reach itself removed it, and an `unreached` claim, which says no target reached the site.
ADR 0025 raises `unstable-baseline` over them, counts them, and stops there.
The report then holds dispositions it says it does not stand behind, and a reader is told to make the suite deterministic and run again.

The dispositions themselves can be established without the moved record.
A proof removed the target; running the mutation against the target puts back what the proof took away.
A kill is existential and rests on no proof, so what needs running is exactly what `report::drift::rests_on` already names.

## Decision

1. **Every disposition resting on a moved target is run again against that target, with its reach recorded, and the run replaces it only where it reached the site.** After the mutations are judged and the drift records folded, and before any later phase reads a disposition, each moved target, in name order, is asked every mutation of the part whose route did not put it to that target and whose disposition is `survived` or `unreached`.
   The run is the whole-target execution a route would have made, with the same arguments, bound and quiet measurement, and the guards record what it reached as a control's do.
   A target that moved is one whose reach is not a function of the code, so a pass says something only about a run that reached the site: a pass whose own record shows the site reached leaves `survived` and adds the target to the route's reaching set; a pass that did not reach it, or could not record, leaves the disposition as it was, still resting on the moved target.
   A kill is confirmed as every kill is, by an original-code control of the target that passes, since a target whose reach moves is exactly where a failure that is not the mutation's comes from; a confirmed kill replaces the disposition with `killed` by that target.
   A run that waited, hit its step bound or errored replaces the disposition with that outcome, a hole rather than an answer.
   A disposition is replaced, never added beside ([ADR 0021](0021-a-claim-is-a-perturbation-an-observer-and-a-decision.md)), so the catalog still holds one decision per mutation.

2. **The replacement is recorded as the pair it is.** Each repair writes one `repair` trace record: the mutation, the moved target, the disposition it had and the one it has now.
   The audit re-derives both halves from the recording alone: that the old disposition rested on a target the `touch` records say moved, and that the new one is what the repair's own execution and `touch` records decide — a pass counts only where its record shows the site reached.
   A report whose disposition differs from the last repair of that mutation, or a repair of a mutation that rested on no moved target, is a violation.

3. **The finding counts what still rests, and a moved target nothing rests on is a limitation.** `unstable-baseline` is raised for a moved target while any disposition still rests on it — one the repair could not run, in a part that did not see the move, or after an interruption.
   A moved target every resting disposition of which was decided again with its reach recorded at the site says so as the limitation `reach-moved`, naming the target and how many dispositions were run again: the suite's reach is still not a function of the target, which a reader deciding whether to trust later runs needs to know, but nothing this run concludes stands on it.

4. **A part repairs what it holds, and a merge counts what is left.** A shard runs again the dispositions it holds against the targets it saw move.
   A target another shard saw move is moved in the merge, and what this shard holds that rests on it was never run again, so the merge's finding counts it, naming for each moved target how many dispositions no part ran again, and the verdict is `INSUFFICIENT`; that is ADR 0023's fourth way out, unchanged.
   An interrupted run keeps no drift record in its checkpoint (ADR 0025 decision 7), so a resumed run measures and repairs afresh.

## Consequences

- A suite whose reach depends on order no longer leaves survivors a run cannot stand behind: each is killed or survives by an execution.
- A moved target costs one execution per resting disposition; a target that moved in a run that discharged much of the catalog costs as much as the proofs saved.
- `unstable-baseline` now means "something this report concludes still rests on a moved reach", and `reach-moved` means the suite's reach moved and the run paid to establish everything regardless.
