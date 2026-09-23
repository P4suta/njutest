<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0022 — Composition needs two layers answering one question

## Status

Accepted, 2026-09-18.
Refines [ADR 0004](0004-proof-layers-not-budgets.md) and [ADR 0021](0021-a-claim-is-a-perturbation-an-observer-and-a-decision.md), and applies [ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md)'s rule about interfaces with one implementation to proof composition.

## Context

The plan that introduced the claim model named cross-layer composition as its most valuable and most dangerous idea: a proof established in one layer discharging work in another, turning the run into something that compiles an assurance plan rather than executing one.
With two perturbation spaces now in the tree — the source syntax of one process, and the seams that process talks over — it is time to say whether such a composition exists.

It does not, and the reason is worth writing down, because it will keep being true of every pair of layers that does not share a question.

A proof discharges work when it establishes something about *the same claim* another layer would otherwise have to run.
Within the mutation phase this happens constantly: `never-infected` says a target cannot observe a difference,
so that target's execution against that mutation is removed.
Within the seam phase it happens once: an answer with no body comes back byte for byte the same when cut short, so no observer could notice, and the run is removed.

Between the two, there is nothing of the kind.
A mutation asks "if this expression were different, would anybody notice"; a seam question asks "if the dependency answered differently, would anybody notice".
Neither establishes anything about the other's subject.
A target that noticed no mutation is not thereby a target that notices no seam fault — it might assert on nothing the mutations touched and everything the dependency returns.
The reverse fails the same way.

The plan's own list of composition rules bears this out.
Every rule it names is one of three things: already in place within a single layer (the type system's kills, the seam's byte-identical answers), blocked on a tool this repository does not require (Kani, which [ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md) keeps optional), or about measuring one perturbation space at two scopes, which this product does not do.

## Decision

1. **No composition framework ships without two rules to put in it.** An interface with one implementation is a promise nobody asked for; a composition interface with no sound rules is worse, because the shape of it invites somebody to add an unsound one later and the machinery will make it look supported.

2. **A composition rule is admissible only where both layers decide the same claim.** Same perturbation, same identity, two observers — or same observer,
   two ways of establishing what it would do.
   Anything else is an analogy, and an analogy that removes a run reports a gap as assured.

3. **The audit comes before the rule, not after it.** When a second rule does appear, `xtask compose-audit` and the ON/OFF differential are built first,
   as ADR 0004 requires of every layer.
   Composition buys speed, never correctness, so it is never the thing that is allowed to go in untested because it is urgent.

## Consequences

The cost of two perturbation spaces is the sum of the two, and this is the right answer rather than a missing optimisation.
ADR 0004 already says a run that is asked for two things pays for two things; nothing about a seam question was made cheaper by the mutation phase having run, because the mutation phase established nothing about seams.

What the two layers do share is the report.
A reader sees one verdict over both,
`Decision` partitions both catalogues the same six ways, and `njutest report --format spec` annotates what the seams observed with who held it up.
Composition of *evidence for a reader* is not composition of *work*, and conflating the two is how a product ends up removing runs it needed.

This ADR is what a later change has to argue against.
Reopening it means naming two layers that decide the same claim, and showing an audit that re-derives the composition from the recording without calling the code that performed it.
