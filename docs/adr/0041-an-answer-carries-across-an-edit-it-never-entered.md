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
   - it is not an `async fn`, and its return type holds no `impl Trait`: calling it builds a future without entering it, and the type its body decides can be observed without entering it.
   - it is not in a unit whose kind is `proc-macro`, whose bodies run inside the compiler and never inside a test.
   - it declares no item (a `fn`, type, `impl`, `trait`, `use`, `mod`, macro, `const` or `static` inside it changes the program where nothing enters the body), and holds no inline `const { … }` block;
   - the arguments of every listed macro are read as tokens, and any macro invoked inside them is held to the same rule;
   - no external glob import and no `#[macro_use] extern crate` appears anywhere in the unit, which would let a macro reach the body under a listed name.
     The exact lists are in `docs/engine/carry.md`, which is what the audit implements, not the engine's code.
3. **Skeleton.** A unit (package name, target name, kind, test flag; never a package id, which carries an absolute path) has a skeleton digest.
   It covers every file the unit's dep-info names, with each sealed body replaced by a placeholder naming its item and the shape of its lines, so an edit that moves the lines after it (and `line!()`, a panic's `Location`, a backtrace) moves the skeleton, and the environment variables rustc recorded reading, and the generated files, and the output of the build scripts it depends on.
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
     A process of the program that started without the variable that asks it to record, as a test's child started with `env_clear()` does, leaves a mark the first time it enters any item; an execution that overlaps such a mark in time is not complete, since what that process entered is unknown.
   - P4 (a kill): the killing execution meets P1–P3, its target passed this run's baseline, and its filter names tests this baseline ran.
   - P5 (a survivor): every target and filter this run's route executes has a recorded execution with an equal filter meeting P1–P3.
     A subset of targets is enough; a narrower or different filter is not.
     No target that reaches the mutant may start a process the run cannot see into, since no run of it could claim a survival.
   - P6: a record is written only from an attributable answer: a failing test named, or a signal the process raised itself, and never after cancellation.
   - P7: every target an execution ran held its reach under a control of this run's tree ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)).
6. **Visible.** A carried answer's route says `carried`, and a refusal names one word from a closed set: `skeleton-changed`, `item-changed`, `unsealed`, `entry-incomplete`, `route-grew`, `filter-differs`, `reach-moved`, `uncontrolled`.
7. **Audited before it ships.** The engine audit gains a `carry` layer that recomputes body digests, sealing and skeletons from the pristine sources with its own parser, from `docs/engine/carry.md`, and re-checks every carried row against the record the run believed and kept beside its report: its locus, P1–P3, and P4 and P5 against the plan the run held it to, with each planned target one the guards' record says reaches the mutation.
   P7 is re-derived from the baseline and control records of the run's own recording, for every target a carried answer rests on.
   Planted defects, each found by name before any run is read: a kill carried across a changed entered item, a survival whose route grew, a changed skeleton, and a kill carried through a target whose control reached other than its baseline.
   An edit-pair differential runs a fixture, applies a scripted edit, runs again with carry and with `--no-cache`, and requires the two to agree mutant by mutant and at least one answer to have been carried.

## Consequences

- A CI run of the whole catalog on the default branch costs, roughly, the mutants whose executions entered what changed, and answers for the whole.
  A pull request that touches a little costs little, in one job, without a matrix.
- The layer inherits the premises every stored answer already has: a test is deterministic given the same program and environment, and it reads nothing outside the tree that the key does not name.
  Carrying widens how often those premises are relied on, so their failure modes are named here rather than hidden:
  - **Nondeterminism.** An execution that missed C by chance (a hash seed, the time, thread order) may reach it next time.
    Only answers from targets whose drift standing held ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)) are carried.
  - **Reads outside the tree** (the network, `/etc`, `$HOME`, the runtime environment) are keyed by nothing, today or here.
  - **Reads inside the tree at run time** (`fs::read("tests/data/x.json")`) are in no dep-info, so they are keyed by nothing either; carrying relies on that premise more often than the exact store does.
  - **The selection shapes the skeleton.** Only an instrumented file has placeholders, so a run that catalogs fewer files has other skeletons, and nothing carries between it and a whole run until every workspace file records the items it enters.
  - **The sealing lists are a syntactic lemma.** A standard macro found to expand to an item is removed from the list, which unseals bodies and costs only speed.
  - **The unit graph** comes from cargo's metadata and dep-info, not from its unstable unit graph.
  - **A shared store is trusted** by whoever reads it; a store is written only by runs that could have run the mutant.
- A proof layer that can be wrong about one of these is audited against recordings like every other; the differential is the check that it removes work without changing an answer.

## Amendment, 2026-09-26: a line moves only what runs after it

### Context

Measured on a real workspace (storage-scout, 1202 mutants): one comment line added inside one sealed body carried nothing.
1100 answers were refused `skeleton-changed`, because decision 3's placeholder names the shape of a sealed body's lines, and the tree skeleton every execution is held to folds every unit.
That is sound: a line added moves every position after it, and a position can be observed.
It is also why a second run after "a handful of edits" reuses nothing, since an edit that adds or removes a line is the usual kind.

A position is observed only by code that runs, or by the compiler while it evaluates something.
Code that runs is code an execution enters; every cataloged body the compiler does not evaluate records its entry, sealed or not ([ADR 0027](0027-an-item-is-entered-where-its-body-starts.md)).
So a line that moves need move only what entered code, or compile-time evaluation, can see.

### Decision

1. **Placeholders lose their shape.** A sealed body's placeholder in the skeleton is `{sealed:<entry>#<ordinal>}`: its lines no longer move the skeleton.
2. **The skeleton keeps positions only where the compiler reads them.** Each `$root` file contributes one more entry: the position of every compile-time consumer of a position outside a sealed body.
   A position is a line and a column, counted as the compiler counts them: only a line feed ends a line, a leading byte-order mark is not a column, and a column counts characters.
   The consumers are:
   - every `const fn` body, and every `const` and `static` initializer, at its first token;
   - every expression the compiler evaluates outside a body that holds a macro invocation or a call: an array length in a field, a type alias or a signature, an enum discriminant, a const generic argument or default, and an associated const default;
     a literal or a path cannot observe a position, so it is not one;
   - every item-level macro invocation, every attribute macro and every derive, at the invocation, since the positions of its expansion resolve to the outermost invocation;
   - every documentation code block, since a doctest is named after its line.
     An edit that shifts none of these moves no skeleton.
3. **Every body that runs records its entry.** A body that is not mutated records its entry all the same, with a marker and no candidate: a `#[test]` function, an item under `#[cfg(test)]`, a file the configuration excludes, and a workspace package the run left out of its selection.
   Each is already parsed; each can run whenever a test calls into it; and an entry marker is the only way to know that it did without a premise about what its code reads.
4. **An entered body is held where it starts.** Every entered item of an execution records the position of its body's opening brace beside its body digest.
   A new premise, P8: every item an execution entered starts where it started, or the answer is refused `item-moved`.
   With decision 3 this covers every body that runs, sealed or not, mutated or not: a panic's location, `line!()`, `Location::caller()`, a location a dependency's `#[track_caller]` function captures into a value (`error-stack`, `snafu`), a snapshot macro's position, and a backtrace frame are each read by code that is running, and that code is either an entered body, whose own positions are then unchanged, or code outside the tree, which no edit moves.
5. **The engine never reads what libtest records of a test's position.** `TestDesc` holds a start and an end line for every `#[test]`, built whichever test runs, but it is visible only through `--list --format json`.
   A law holds that no verdict and no key reads that output; if one ever must, a `#[test]` body keeps its line shape.
6. The refusal words gain `item-moved`.
   The audit re-derives both halves from `docs/engine/carry.md`, with the same position rule and its own parser.
   Planted defects: a kill carried though an entered body moved; a carried answer across a moved `const` initializer that uses `line!()`; an answer carried though a field `[u8; line!() as usize]` below an edited body moved, read by an entered body that did not; a test body that moved and ran; and a documentation code block moved by an edit above it, whose doctest is renamed and must not carry.
   The edit-pair differential gains a scripted line insertion above an entered body and one above a body nothing entered: the first runs again, the second carries, and both agree with `--no-cache`.

### Consequences

- An edit that adds lines inside a body carries every answer whose executions entered nothing below it in that file, and nothing that entered what moved.
  Inserting an item still moves ordinals and signature text, so it still refuses everything in the unit.
- An `async` body, or one whose return type holds an `impl` type, stays unsealed for its layout as before; its positions are observed only once it is polled, which runs its entry marker, so P8 covers them.
- No premise is added: every runtime body of the tree records its entry, so what moved and ran is known rather than argued about.
  A test that moved refuses only the answers of executions that ran it, which is the tests below the edit in its file; integration tests, and tests above the edit, still carry.
- Markers in bodies nobody mutates cost what an entry costs where nothing else is recorded; decision 3 is measured before it ships, as ADR 0027 measured the markers it introduced.
- A macro defined with `macro_rules!` is not a consumer by its definition; its positions are those of its invocation, which decision 2 records.
  A test holds that a `panic!` expanded from a macro defined above an edit and invoked below it reports the invocation's line.

### Future work

A body that moved without changing is refused because a position can reach a verdict through a dependency's `#[track_caller]` function, and whether a call resolves to one is a question of types, not of text.
A build with `-Zlocation-detail=none` and no debuginfo would redact every such position alike on both trees, leaving only `line!` and `column!`, which text can find; that would let most moved bodies carry.
It needs a nightly compiler, it changes the program from what `cargo test` builds, and the redaction would have to enter the build's identity, so it is an opt-in for later rather than a default.
