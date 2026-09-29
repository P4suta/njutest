<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-drifts-sealed

[fixture-drifts](../fixture-drifts/README.md), with a test the sealed target can run.

Natively it is the same suite: its one test looks for a mark in the run scratch above its temporary directory, the baseline finds none and calls `first_visit`, and every process after it finds the mark and calls `return_visit`, so the target's control reaches `return_visit` and not `first_visit` and the target is recorded as moved ([ADR 0025](../../docs/adr/0025-a-reach-that-moves-is-not-a-measurement.md)).
A sealed instance shares nothing with any other and has no temporary directory, so there the test always takes the first path, and its sealed control passes.

The mutations of `return_visit` are `unreached` on the word of the baseline, and sealed: no sealed control reaches them either.
That verdict rests on the moved record, since the native reach it was held to is the baseline's.
`njutest verify` puts each of them again on the sealed bench with the moved target counted among the targets that reach it ([ADR 0036](../../docs/adr/0036-what-rested-on-a-moved-reach-is-run-again.md)).
The bench answers for the target, so the put re-establishes `unreached`, now with the target in its route, and nothing rests on the moved record any more: the run raises no `unstable-baseline`, and names the target in `reach-moved` with the two dispositions it put again.
The engine alone raises nothing, because it never asks a control what it reached; its fates below are what that one baseline record decides.

| Function | Baseline reaches it | Control reaches it | Sealed control reaches it | What a run says |
| --- | --- | --- | --- | --- |
| `first_visit` | yes | no | yes | put to the sealed test, survives |
| `return_visit` | no | yes | no | `unreached`, sealed, put again with the moved target counted |
| `sum` | yes | yes | yes | killed by a sealed execution |

```fates
src/lib.rs:9:5 return-default survived
src/lib.rs:9:7 add-to-sub survived
src/lib.rs:9:9 int-decrement survived
src/lib.rs:9:9 int-increment survived
src/lib.rs:15:5 return-default unreached
src/lib.rs:15:7 mul-to-div unreached
src/lib.rs:15:9 int-decrement unreached
src/lib.rs:15:9 int-increment unreached
src/lib.rs:21:5 return-default killed
src/lib.rs:21:7 add-to-sub killed
```
