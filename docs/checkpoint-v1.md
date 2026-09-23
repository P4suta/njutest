<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Interrupted assurance checkpoint v1

**Status: implemented.** `njutest-assurance-checkpoint-v1` is strict scheduling state for continuing an interrupted verification.
It is not a partial report or a second evidence cache.

One exact input identity owns one file:

```text
<user cache>/njutest/outcomes-v1/checkpoints/<identity>/checkpoint-v1.json
```

Only a named kill can enter `mutants`.
The Rust wire type is a closed `SavedDisposition::Killed { by }`; it has no step-limit, timeout, survival,
error, or un-attributed kill state.
A successor may therefore inherit only an existential fact a target established about the identical tree.
Every other mutation is judged again.

`drift` keeps every drift record a control established before the interruption, as the report spells one ([report v1](report-v1.md#drift)).
An inherited kill is not confirmed again, so the control that confirmed it does not run again either, and what it established about its target's baseline reach would otherwise be lost: a resumed run folds the saved records with its own, and a move either run saw stands.
A checkpoint written before this field existed is read as holding no record, so every target it measured reads as `not-measured` unless this run compares it again: less is claimed, never more.

Historical `checkpoint-v1.json` files are outside this layout.
In particular,
their `runaway` and `timed_out` strings carried no matched control and are never reinterpreted as detections.

Saved baseline targets retain the files they reached, not narrower regions,
so a resumed run routes them at file granularity and can execute more work but never less.
A completed run removes its checkpoint; `--no-cache` reads and writes none.
