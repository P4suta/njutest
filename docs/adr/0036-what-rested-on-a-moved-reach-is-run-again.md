<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0036 — What rested on a moved reach is run again

## Status

Proposed, 2026-09-24.
The repair [ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md) decision 5 left for the next change.

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
