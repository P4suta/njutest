<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0038 — A read-back row carries its answers

## Status

Accepted, 2026-09-25.
Decides what a mutation row read back from the evidence store, or inherited from a checkpoint, says about the targets that were asked.

## Context

`hollow-target` is decided over the whole catalog from `routing.answered`: the targets actually put to each mutation and what each said.
Only a row this run established filled that list.
A row read back from the evidence store carried an empty one, and a kill inherited from a checkpoint carried no routing at all.

So the same catalog over the same tree drew different findings depending on what was cached.
On `fixture-hollow`, a cold run names `smoke` as hollow; a second run that reads every answer back names nothing, and a finding is what moves a verdict.
A warm cache and a resume changed the conclusion, which is a verdict resting on how the run measured ([ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md)).

Three directions were weighed.

- **Let the whole-catalog decision skip or flag rows it cannot judge.** A per-row closed state — asked, read back, resumed — would make the gap visible, but whatever the decision does with an unjudgeable row still differs from what it does with the asked one.
  Refusing to clear a target keeps a warm run permanently short of a verdict; stating a limitation instead lets a warm run conclude where the cold one could not.
  Either way the verdict still depends on the cache.
- **Ask again whenever a row's answers are needed.** That is the store not being used.
- **Keep the answers with the disposition.** A survival already names every target it was asked of, each with its behaviour key, and is believed only when this run routes no target outside them; every one of those answered `survived`.
  A kill named only the target that noticed.
  The targets asked before it, and what they said, were the part that was lost.

## Decision

A kill carries the answers given before it, wherever it is kept.

- The evidence store's `Killed` record carries `before`: every target asked before the killer, in order, each with its behaviour key and outcome.
  It is believed only when those are exactly the targets this run would ask before the killer, in the same order — a run asks the targets its route reaches in name order and stops at the first confirmed kill — each with the same key and seen to pass by this run's baseline.
  A record naming them in another order, or holding a kill among them, is one no run wrote and is unreadable: a kill that did not reproduce is recorded as unconfirmed and the run moves on.
  A target this run would ask that the record has no answer from refuses reuse as `target-entered`; one the record answered for that this run would not ask refuses it as `not-routed`.
- The checkpoint's `SavedDisposition::Killed` carries the same `before`, by target name, and a `before` that names the target that noticed, or holds a kill, is not a checkpoint.
- A read-back or resumed row reports those answers followed by the kill; a read-back survival reports every target this run routes, each `survived`.
- `Routing::of` takes the answers, so no path builds a row's routing without saying what was answered.

`routing.answered` therefore means the targets asked about the mutation, by this run or by the run the row came from, and `reuse` names which.

## Consequences

The same catalog over the same tree reaches the same `hollow-target` findings cold, warm, and resumed; `toolchain_verify` holds both the read-back and the resumed case against a cold run on `fixture-hollow`.

A kill record written before this decision has no `before` and is unreadable, so it is refused and asked again, which costs one run its time and no run its verdict.
A checkpoint written before it is refused as corrupt, as any checkpoint of another shape is; removing it, or `--no-cache`, starts the run over.

A kill is now reused under a narrower condition than before: a target that entered the route ahead of the killer refuses it.
That is the condition under which asking again could have given an answer the record does not have.
