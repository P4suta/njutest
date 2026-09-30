<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-const-items

Const item initializers are changed by a separate compilation of each mutant under the `compiled` tier.
The tests observe the compiled value of free and associated constants.
`UNUSED` survives because no assertion reads it, and multiplication in place of division is equivalent here.
Incrementing `LIMIT` or making the denominator zero is refused by the compiler.

## Fates

```fates --tier compiled
src/lib.rs:0:0 int-decrement refused
src/lib.rs:0:0 int-increment refused
src/lib.rs:7:25 int-decrement killed
src/lib.rs:7:25 int-increment killed
src/lib.rs:10:27 true-to-false killed
src/lib.rs:13:25 int-decrement survived
src/lib.rs:13:25 int-increment survived
src/lib.rs:16:23 int-decrement killed
src/lib.rs:19:27 int-decrement killed
src/lib.rs:19:27 int-increment killed
src/lib.rs:19:29 div-to-mul survived
src/lib.rs:19:31 int-increment killed
src/lib.rs:26:28 int-decrement killed
src/lib.rs:26:28 int-increment killed
```
