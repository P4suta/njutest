<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Selecting the tests a change can reach

**Status: implemented.** `njutest measure` and `njutest select`; what is not yet measured is listed at the end, and each such gap runs everything.

`njutest select` says which test targets can notice what changed since the tree was measured, and proves the rest cannot.
It skips a target only where the proof holds; anything it cannot place runs everything.
There is no threshold, sample, or prediction in it.

```console
$ njutest measure
MEASURED	12 targets	11 held	840 items	312 files
KEPT	reports/reach
$ njutest select
SELECT	3 of 13 targets run	10 proved unable to notice this change
RUN	demo/test/parsing	its tests entered 1 changed items
RUN	demo/doc/demo	the measurement holds nothing about it
RUN	demo/test/flaky	a second run of it reached something else
SKIP	demo/lib/demo
…
$ cargo nextest run -E "$(njutest select --format nextest)"
```

nextest does not run documentation, so `--format nextest` prints the filterset on stdout and, on stderr, one `DOCTESTS` line for every doc target the selection runs: a CI that only runs nextest still has to run those with `cargo test --doc`.

## The argument

Two programs identical except at *p* run identically until *p* is first reached ([ADR 0014](adr/0014-the-guards-are-the-measurement.md)).
A test that never entered the item holding *p* on the measured tree therefore runs the same on the changed one, and running it asks nothing the change could answer.
`measure` records, for every target, which items anything of it entered ([ADR 0026](adr/0026-an-item-is-entered-where-its-body-starts.md)), and runs each target whole a second time to show that what it entered is a function of the target and not of the run ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)).
A target whose second run reached something else, or could not be compared, is never skipped.

Entering is not the only way a test depends on a file: a test that reads `src/quiet.rs` as text depends on it without running a line of it.
So `measure` runs each target a third time with every source file that holds an item taken out of the copy, and puts them back after.
A target that answers differently without them — a test fails, or another set passes — reads the tree, is `reads-tree`, and is never skipped.

## What a change is

A selection never asks git what changed.
`measure` keeps the digest and execute bit of every file of the tree, read by the rules a run copies it by, and the measured bytes of every source file.
`select` reads the tree again by the same rules and compares.
A file git ignores, an uncommitted edit, a filter, a file made runnable: each is a change, because each is a difference in what a build or a test could read.

Each changed source file is read in both versions ([ADR 0027](adr/0027-a-change-is-placed-by-reading-both-versions.md)).
A change is placed in an item only where it lies strictly inside the body of a measurable item, the rest of the file is the same tokens, and the change does not reach past the body.
An item whose tokens moved to another line or column is changed too, because a panic inside it says where it is.

## What runs everything

Every target runs when any of these differs from the measurement, and `select` says which:

- the toolchain, the build selection, the harness arguments, or the targets left out;
- a variable the run selects for its test processes, or one the compiler read through `env!`;
- the rules the tree is read by;
- a file the build read outside the tree;
- a manifest, lock file, build script, toolchain file, or cargo or njutest configuration;
- a file compiled into a procedural macro or a build script, wherever it is and whatever it is named, because that code runs inside the build and decides what other targets compile to;
- a file that holds no measured item, or was added or removed;
- a change outside every measured body: an item, a signature, a type, a `use`, an attribute;
- a changed body that gains an `impl`, an exported symbol, or a macro outside the standard expression macros, or a `use` that could shadow one;
- a moved item that a foreign derive or attribute macro reads, or a moved `const fn` or `static`.

A target that exists now and was not measured runs.
The doc target is never measured, so it always runs.

## What it does not do

A selection says which targets need not run, never which need not compile: build everything, then run what it selected.
`measure` is one instrumented build and two runs of every target; it is worth paying once and reusing for as long as nothing above moves.
A test that reads a source file only when one is missing, or reads it without its answer depending on the text, is not told apart from one that does not read it at all; the run without the files finds a test whose answer changes when the files are gone.
A child process started with a cleared environment is noticed and its target runs ([ADR 0028](adr/0028-a-process-that-loses-the-environment-says-so.md)).
