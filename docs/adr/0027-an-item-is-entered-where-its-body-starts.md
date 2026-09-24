<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0027 — An item is entered where its body starts

## Status

Accepted, 2026-09-24.
Implemented by the entry markers of the `instrument` module of rust-mutants, the `e` record of `touch`, and the `entry` layer of `cargo xtask engine-audit`.
The measurement `njutest select` will route a change by; see [item reach](../engine/item-reach.md).

## Context

`select` skips a test when a change touches only items the test never entered.
The argument is [ADR 0014](0014-the-guards-are-the-measurement.md)'s: two programs identical except at *p* run identically until *p* is first reached, so a test that never reached *p* runs the same on both.

The guards record reach at mutation sites.
A site is not the first thing a body does, and a body need not have one:
a test that enters `halve` and panics on its first statement reaches no site of `halve` and ran code in it all the same.
Treating "reached no site of *X*" as "never entered *X*" would skip that test on a change to `halve`'s first statement, and the test is the one that change can break.

What the argument needs is an event that happens before any byte of a body runs, and happens every time one does.

## Decision

**The first statement of every body the instrumenter already rewrites records the item it belongs to.** That is every function, method, and trait default method with a body — test code included — at the point where the step checkpoint of [ADR 0011](0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md) already stands, so the set of bodies is one the instrumenter already proves it can write into.

**A closure or an `async` block records the item it is written in.** Its body runs when it is called or polled, which can be after the item returned and on another test's thread.
Without a marker of its own, a test that calls a closure some other test created would run code written in *X* without having entered *X*.
With one, "entered *X*" means "ran code written inside *X*'s body", which is the claim a change to those bytes needs.

**A `const fn`, a `const`, and a `static` are cataloged as unmeasurable.** A marker cannot be called in a const context, and what the compiler evaluates at compile time no test enters.
They stay in the catalog, with `measurable: false`, so that the innermost item holding a change is always the right one: a change inside a `const` nested in a function belongs to the `const`, and `select` routes it to every test rather than to the function's.

**Items have their own index space**, dense over the whole tree in path order, so that a marker's index names one item of the tree and the runtime sizes its per-thread record by the file's own window, apart from the mutants'.

**A marker records only in touch mode.** Off, it is one load of the `OnceLock` that already gates the site record, and a branch.

**An item a thread enters after its own record is gone is recorded as entered by every test**, not refused.
A thread-local's `Drop` can call into the program after the runtime's thread-local is destroyed; refusing there would cost the whole target its measurement, and widening only makes `select` run more.

**The claim is held by an audit that does not ask the engine.** The `entry` layer re-derives that every reached site and every kill lies in an item the reaching or noticing test entered, from `touched-v1.json` and the rows alone.

## Consequences

- `touched-v1.json` gains `items` and each target's `entered`; the touch log gains the `e` record; the `touch` trace event gains `entered`.
  All are v1 documents that have not been released, so the version does not move.
- On `fixture-item-reach`, `a_word_is_refused` is in the entered set of `halve` and in the reach set of none of its sites.
- The cost, on `fixture-families` (291 mutants, 18 items), alternating the engine from `main` and this one three times,
  each run with `--no-cache` and a cache directory of its own, on a machine whose one-minute load stood between 4.3 and 4.6.
  Phase durations are the recording's own, in milliseconds, as the median of the three runs and their range:

  | | instrumented build (`validate`) | baseline and measurement (`verify`) | whole run |
  | --- | --- | --- | --- |
  | before | 264 (200–280) | 394 (324–417) | 3,308 (3,089–3,349) |
  | after | 261 (259–264) | 423 (417–439) | 3,359 (3,308–3,365) |

  The differences are inside the spread of either side on a fixture this small; the markers add no process, no build, and no pass, only one call per body.
- A marker is a call on the line that already holds the step checkpoint, so no line moves and every position a report prints is where it was.
- A should_panic test was named with libtest's ` - should panic` suffix, so its thread matched no test the baseline ran and everything it reached was attributed to every test.
  The engine now names it by its name; this came up because the first fixture exercising entry used one.

- **Every first sighting is written when it happens.** The runtime used to hold a named thread's records back in batches of 64 and write the rest when the thread's thread-local dropped, and a named thread still alive when the process exits — a pool worker, a detached `Builder::new().name(..)` worker — never drops it, so what it entered and reached was never written.
  For routing that read as "no test reached this", a reported hole; for a proof it would have read as "nothing ever differed" or "no body was entered", which discharges a mutation a test kills; and for `select` it would read as "no test needs this item".
  A record is now written the first time a thread sees an index, the same lines a batch would have written once, so a run writes no more than before and loses nothing it saw (`fixture-item-reach`'s `thrice`).
- **Entered is not depended on.** "Test T entered item I" means T ran code written inside I, exactly, and nothing more.
  A test that reads a value another test's run of `init` cached in a static never enters `init`, and whether it depends on `init`'s body is a question of which test ran first, not of what either entered.
  So nothing reads entry as dependence: `select` narrows by it only per target, whose run includes whichever test initialised the value, and narrows to single tests only where the run has established that a test on its own answers what its target answers.
- **The `entry` audit layer is not independent of this measurement.** It holds sites against entries that the same runtime wrote, so a record lost before it was written drops both and the two agree.
  What holds the measurement from outside is a run that routes nothing — every mutant against every test — whose kills must each lie in an item the killing test entered; that differential is `select`'s acceptance, not this layer's.

## Alternatives

- **Infer entry from the sites.** That is the gap this closes, and ADR 0014 already refused the same inference for branch bodies.
- **An LLVM coverage build.** It answers "which regions ran" per target, not per test, and is the second full build ADR 0014 removed from the default run.
- **Markers only in bodies with no site before their first statement.** Smaller, and it would be a claim about the walker's site rules held by nothing; the audit would have to re-derive those rules to check it.
- **No markers in closures.** Cheaper, and unsound for a closure that escapes its item, which is an ordinary shape (a callback stored in a registry, an iterator adaptor returned from a function).
