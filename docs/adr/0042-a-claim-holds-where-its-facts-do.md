<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0042 — A claim holds where its facts do

## Status

Accepted, 2026-09-26.
Implemented for expectations by `facts::{Predicate, Facts}`, `Session::{unread, holds, facts}`, `run::{Where, Unheld, Standing::Inapplicable}`, the run report's `facts` and `where`, `MergeError::TargetsDisagree`, and engine-audit's `held` rule.
Configured skips take the same resolution with the skip anchors that follow, and the declared environment enters the outcome key and carry's premises in the change that first reads it.

## Context

An expectation (`[[mutation.expect]]`, [ADR 0017](0017-an-acceptance-is-a-claim-a-run-can-refuse.md)) is a claim about what a run establishes of one mutation, and the run refuses it when it establishes something else.
That is right when every run of the tree can establish the same thing, and wrong when the thing depends on where the run is.

storage-scout met both shapes of it, at rust-mutants `0821533b`:

- `links_of` returning `metadata.nlink()`: on APFS a clone-and-swap refuses a file with several links, so a test observes the count and kills the `return-default`; on btrfs, which the repository's CI measures, sharing in place never refuses such a file and the mutant survives, which is what the repository claims; on ext4 the tests that reach it skip themselves unless `STORAGE_SCOUT_REQUIRE_SHARING=1`, so it is not run.
- `Session::refresh` branching on `self.watcher.recursive()`: inotify never watches a subtree, so `condition-to-false` survives on Linux and is killed on macOS, and its mirror `condition-to-true` does the reverse.

So every run on a developer's machine reports some of the repository's claims `stale`, and a claim that can only ever be met on one platform cannot be written at all for the mirror mutant.
The noise is worse for files the build never compiles: a claim on `events_macos.rs` is `unmatched` on Linux and one on `share_linux.rs` is `unmatched` on macOS, because the compiler never accepts a mutant in a file no unit of the build read.

A claim whose standing depends on where it is judged teaches its readers to ignore standings, which is the one thing a claim exists to prevent.

## Decision

A claim is judged only where the facts it was established under hold, and the report says which facts those were.

**A file the build did not compile makes its claims inapplicable, with nothing written.** Discovery walks only the files the build's units read, by their dep-info, so a file the build did not compile holds no candidates, and a claim on it cannot be told from a claim whose locator rotted by the catalog alone.
So a file a claim names and no unit read is parsed on its own, with the same per-file walk discovery uses, and the locator is resolved against what that walk finds:
a file that does not exist, or a locator that names nothing in it, is `unmatched` on every host, so a claim whose locator rotted is caught everywhere and not only on the one runner where it would apply;
a locator that names something in it is `inapplicable`, and the report gives the fact as `not compiled for <target>`.
A claim on a file a unit read is resolved against the catalog, as today, and then by its `where`.
Configured skips are resolved the same way, and a run pays one parse for each claimed file its build did not compile.

**Behaviour that differs inside compiled code is declared with `where`.**

```toml
[[mutation.expect]]
path = "crates/cli/src/watch.rs"
item = "Session::refresh"
rule = "condition-to-false"
original = "self.watcher.recursive()"
outcome = "survived"
where = { cfg = 'target_os = "linux"' }
reason = "inotify never watches a subtree, so recursive() is false on every Linux run and the branch it guards is the one taken"
```

`where.cfg` is a Cargo `cfg` predicate — `all`, `any`, `not`, `name`, `name = "value"` — over the names a target alone decides: `target_*`, `unix`, `windows`, `target_pointer_width`, `target_endian`, `target_has_atomic` and `panic`, as `rustc --print cfg --target <target>` from the run's own toolchain prints them.
Any other name is a configuration error that names it, because `debug_assertions`, `test`, and every `--cfg` a build's flags add are not facts a probe of the target can report, and a predicate over a fact nobody measured is a claim nobody can check.
`where.env` names exact values in the environment the engine gives the tests, which the engine is handed rather than reads.
Every name `where.env` asks about is an input to the outcome store's key and to carry's premises ([ADR 0041](0041-an-answer-carries-across-an-edit-it-never-entered.md)), because the user has declared that an answer depends on it.

**An inapplicable claim marks nothing and refuses nothing.** It is a fourth standing beside `met`, `stale` and `unmatched`: the mutation is reported as though no claim named it, and no `stale-expectation` is raised.
Two claims may name one mutation when no run can make both applicable; a run in which both apply refuses it with `RM0004`, as today.

**The report records the facts it judged by.** The run report carries the sorted cfg set and every name `where.env` asked about with its value or its absence, and each claim's standing names the fact that did not hold.
`cargo xtask engine-audit` re-evaluates every claim's applicability from those records with its own evaluator, and a planted claim whose recorded facts contradict its standing is a violation it must find.

## Consequences

- storage-scout writes one claim per platform-specific fate and none for a file its build does not compile; a run on any of its three machines reports its claims `met` or `inapplicable`.
- A claim that holds only under a variable is keyed by it, so an answer measured with the variable set is never read back in a run without it.
- The ext4 fate in the first case still needs a decision of its own: a test that reaches a mutation and then declares it cannot measure it here is reported today as a pass, which makes the mutation a survivor.
  That is a question about what a pass means, not about where a claim holds, and it gets its own record.

## Alternatives

- **Probe the whole cfg set the build uses.** `debug_assertions`, `test` and the build's own `--cfg` flags would need the build's rustflags and profile passed to the probe, and every mismatch between the probe and the build would be a claim judged against a set the mutants never ran under.
  Restricting the names to what the target decides covers every case met so far and cannot disagree with the build.
- **Detect filesystem properties.** The engine cannot know which property of a filesystem a test depends on; the repository can, and names it with the variable its tests already read.
- **Leave an inapplicable claim's locator unresolved.** A locator that rotted would then be silent on every host but the one where it applies.
