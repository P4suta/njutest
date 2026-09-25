<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0041 — An answer carries across an edit it never entered

## Status

Proposed, 2026-09-26.
To be implemented in three parts: the evidence (item body digests, sealing, unit skeletons), the entered-item union of every mutant execution, and the rule with its audit.
It refines [ADR 0004](0004-proof-layers-not-budgets.md) and [ADR 0007](0007-survived-evidence-is-universal.md): it is one more proof layer, the reuse layer, stated and audited like the others.

## Context

The outcome store answers a mutant only when nothing the build read has changed: its key folds the whole compiled closure.
That is sound and coarse.
A one-line edit anywhere in a crate misses every stored answer about that crate, so a CI run on the default branch after a small change costs what a cold run costs.

The industry's answers are to measure only the diff, or to split the catalog across more machines.
Measuring only the diff answers for the diff, not for the whole; splitting multiplies jobs with the size of the change and lowers no mutant's cost.
Neither is the answer this project exists to give.

What a run already records says more than the closure does.
A mutant execution enters some items of the program and not others ([ADR 0027](0027-an-item-is-entered-where-its-body-starts.md)).
An edit to the body of an item the execution never entered cannot change what that execution did, provided nothing outside that body changed and the edited body contributes nothing to the program except when entered.

## Decision

1. **The lemma (engine).** Let two trees T0 and T1 have equal *skeletons* for every unit a target links, and differ only inside the bodies of a set C of *sealed* items.
   Apply mutant m to both.
   An execution x (a target, with an exact test filter) behaves identically on T0 + m and T1 + m until it first enters an item of C.
   So if x on T0 + m entered no item of C, x on T1 + m reproduces its result.
   The set that is checked is the union of items the whole process entered, never one test's: a test can run code another test initialised.
2. **Sealed.** A body is sealed only if everything it contributes to the program is its own execution:
   - every macro it invokes is a single segment or `std::`/`core::`/`alloc::`-qualified, and its last segment is in a closed list of standard macros that expand to no item;
     `include*`, `env`, `option_env` and `asm` are not on the list;
   - no file of the unit declares or imports a macro with a listed name, and no glob import from a non-standard path reaches the file;
   - the item and every attribute inside it are in an allow-list (`inline`, `cold`, `must_use`, `doc`, `cfg`, lint attributes, `track_caller`, tool attributes);
   - it is not a `const fn`, a `const` or a `static`, whose bodies can be evaluated where nothing enters them.
     The exact lists are in `docs/engine/carry.md`, which is what the audit implements, not the engine's code.
3. **Skeleton.** A unit (package name, target name, kind, test flag; never a package id, which carries an absolute path) has a skeleton digest.
   It covers every file the unit's dep-info names, with each sealed body replaced by a placeholder naming its item, and the environment variables rustc recorded reading, and the generated files, and the output of the build scripts it depends on.
   Paths are named by the prefix classes `$root` and `$target`, so a skeleton travels between checkouts.
   Anything outside a sealed body is in the skeleton, so an edit to a signature, a type, a constant, a trait impl header, a macro, or an unsealed body changes it.
4. **What a record holds.** A carried answer is filed under a *locus* key that does not fold the closure.
   The locus key is the rule and its version, the mutated item's position and body digest, the offsets and text of the edit, and everything the exact key holds except the closure (toolchain, manifests, lock file, arguments, bounds).
   The record lists every execution the answer rests on: target, filter, the target's skeleton, the items the process entered with their body digests, and whether the union is complete.
5. **When a record is believed.** Every premise must hold; if any cannot be established, the mutant runs.
   - P1: each execution's target has the recorded skeleton now.
   - P2: every item that execution entered has the recorded body digest now, and is still sealed.
   - P3: the union is complete.
     An execution stopped by a clock, a signal, a cancellation, or an unreadable log is not; one stopped at its first failing test is complete for its kill and not for a survival.
   - P4 (a kill): the killing execution meets P1–P3, its target passed this run's baseline, and its filter names tests this baseline ran.
   - P5 (a survivor): every target and filter this run's route executes has a recorded execution with an equal filter meeting P1–P3.
     A subset of targets is enough; a narrower or different filter is not.
   - P6: a record is written only from an attributable answer: a failing test named, or a signal the process raised itself, and never after cancellation.
6. **Visible.** A carried answer's route says `carried`, and a refusal names one word from a closed set: `skeleton-changed`, `item-changed`, `unsealed`, `entry-incomplete`, `route-grew`, `filter-differs`.
7. **Audited before it ships.** The engine audit gains a `carry` layer that recomputes body digests, sealing and skeletons from the pristine sources with its own parser, from `docs/engine/carry.md`, and re-checks P1–P6 for every carried row.
   Planted defects, each found by name before any run is read: a kill carried across a changed entered item, a survivor whose route grew, a change to a `const` that the skeleton missed.
   An edit-pair differential runs a fixture, applies a scripted edit, runs again with carry and with `--no-cache`, and requires the two to agree mutant by mutant and at least one answer to have been carried.

## Consequences

- A CI run of the whole catalog on the default branch costs, roughly, the mutants whose executions entered what changed, and answers for the whole.
  A pull request that touches a little costs little, in one job, without a matrix.
- The layer inherits the premises every stored answer already has: a test is deterministic given the same program and environment, and it reads nothing outside the tree that the key does not name.
  Carrying widens how often those premises are relied on, so their failure modes are named here rather than hidden:
  - **Nondeterminism.** An execution that missed C by chance (a hash seed, the time, thread order) may reach it next time.
    Only answers from targets whose drift standing held ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)) are carried.
  - **Reads outside the tree** (the network, `/etc`, `$HOME`, the runtime environment) are keyed by nothing, today or here.
  - **The sealing lists are a syntactic lemma.** A standard macro found to expand to an item is removed from the list, which unseals bodies and costs only speed.
  - **The unit graph** comes from cargo's metadata and dep-info, not from its unstable unit graph.
  - **A shared store is trusted** by whoever reads it; a store is written only by runs that could have run the mutant.
- A proof layer that can be wrong about one of these is audited against recordings like every other; the differential is the check that it removes work without changing an answer.
