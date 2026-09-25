<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-drifts

A suite whose reach is not a function of the target.

Its one test looks for a mark beside the temporary directory it was given.
A mutation run gives every process of one run a temporary directory of its own inside one run scratch, `rm-scratch-*`, and removes the scratch when the run ends,
so that parent is the one place every process of a run shares and no process of another run sees.
The first process to look — the baseline — finds no mark, leaves one, and calls `first_visit`.
Every process after it finds the mark and calls `return_visit` instead.
Outside a run scratch the test leaves nothing anywhere and always takes the first path.
Both paths pass, and both assert on `sum`, so its mutations are killed and each kill is confirmed by an original-code control of the whole target.

That control reaches `return_visit` and not `first_visit`, over the same passing test the baseline passed.
The baseline's record says the reverse, and every proof read off it is read off a run that could not recur:
the mutations of `return_visit` are `unreached` on its word, and those of `first_visit` are put to a test that no longer calls it and survive.
`njutest verify` records the target as moved ([ADR 0025](../../docs/adr/0025-a-reach-that-moves-is-not-a-measurement.md)) and runs the two `unreached` claims resting on it again against it, with its reach recorded ([ADR 0036](../../docs/adr/0036-what-rested-on-a-moved-reach-is-run-again.md)).
That run reaches `return_visit`, whose value the test does not assert on, so both survive by an execution rather than being unreached on the word of a baseline the control contradicted.
Nothing rests on the moved record any more, so the run raises no `unstable-baseline` and names the target in `reach-moved` instead.
The engine alone raises nothing, because it never asks a control what it reached; its fates below are what that one baseline record decides.

| Function | Baseline reaches it | Control reaches it | What a run says |
| --- | --- | --- | --- |
| `first_visit` | yes | no | put to the test, survives |
| `return_visit` | no | yes | `unreached` |
| `sum` | yes | yes | killed, and the kill confirmed |

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
