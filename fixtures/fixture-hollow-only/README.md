<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-hollow-only

One function and three targets, and one finding: a hollow target, with nothing surviving.

`tests/pins.rs` pins every answer `sign` gives, so every mutation of it dies.
`tests/a_smoke.rs` calls `sign` and asserts nothing about what came back.
Targets are asked in name order, so `a_smoke` answers about each mutation before `pins` kills it, and notices none of them: that is a `hollow-target` finding, and the only one.
The library's own test reaches none of `sign`, so it is neither asked nor hollow.

`fixture-hollow` draws the same finding beside survivors, so a run that lost the finding would still conclude `INSUFFICIENT` there.
Here the hollow target is the whole reason the run is `INSUFFICIENT`, so a conclusion that lost it — as a merge of shards once did — concludes `ASSURED` and the test measuring it whole and in shards sees the difference.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `sign`: the comparison and the two returns |

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 condition-to-false killed
src/lib.rs:8:8 condition-to-true killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge killed
src/lib.rs:8:12 int-increment killed
src/lib.rs:8:16 return-default killed
src/lib.rs:8:16 string-to-empty killed
src/lib.rs:8:36 return-default killed
src/lib.rs:8:36 string-to-empty killed
```
