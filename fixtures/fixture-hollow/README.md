<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-hollow

Two functions and two targets. The library's own test pins both answers
`sign` can give, so every mutation of `sign` dies. `tests/smoke.rs` calls
`double` and asserts nothing about what came back, so every mutation of
`double` survives it.

That makes `smoke` a target this run puts to mutations and that notices none
of them, which is what a `hollow-target` finding is. The library target is
not one: it noticed something, so no finding names it.

The two are separated on purpose. A target that reaches nothing would be a
different fact — a run establishes nothing about a target it never asks — and
a fixture that confused the two would let `hollow-target` pass a test it
should not.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `sign`: the comparison, the two returns; `double`: the arithmetic and the literal |

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge not_run
src/lib.rs:8:12 int-increment killed
src/lib.rs:9:9 return-default killed
src/lib.rs:9:9 string-to-empty killed
src/lib.rs:11:9 return-default killed
src/lib.rs:11:9 string-to-empty killed
src/lib.rs:17:5 return-default survived
src/lib.rs:17:7 mul-to-div survived
src/lib.rs:17:9 int-decrement survived
src/lib.rs:17:9 int-increment survived
```
