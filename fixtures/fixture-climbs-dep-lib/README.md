<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-climbs-dep-lib

The library `nested/fixture-climbs-dep` reads through a path that climbs twice.
It is a fixture of its own so that the climb lands on something real, and it is never measured on its own: nothing mutates a dependency it only reads.

It sits here rather than beside the tree that reads it, because a climb of one level is the case `fixture-outside-dep` already covers.
A copy that placed the tree and its neighbours by two separate rules kept a one-level climb resolving by coincidence and lost every longer one; this pair is the shape that told the two rules apart.

## Fates

A run of this fixture on its own reaches every mutation and decides none of them: the library has no tests, so nothing speaks and the run says so rather than calling silence a survivor.
What it is for is measured through `nested/fixture-climbs-dep`, which reads it.

```fates
src/lib.rs:9:5 return-default unreached
src/lib.rs:9:7 mul-to-div unreached
src/lib.rs:9:9 int-decrement unreached
src/lib.rs:9:9 int-increment unreached
```
