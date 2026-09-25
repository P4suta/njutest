<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-guarded-or

One condition, `over(left) || over(right)`, whose operands are calls a run cannot evaluate a second time, and one test that only ever holds the left operand false.

A mutation of the whole condition has to replace all of it.
If the guard that keeps the original out of the way while a whole-condition mutant is live covered only the left operand,
`condition-to-false` would leave `over(right)` deciding and the test would pass;
this fixture is what says a whole-condition mutant is killed here.

## Fates

```fates
src/lib.rs:8:5 return-true killed
src/lib.rs:8:11 gt-to-ge survived
src/lib.rs:8:13 int-decrement survived
src/lib.rs:8:13 int-increment killed
src/lib.rs:13:8 condition-to-false killed
src/lib.rs:13:8 condition-to-true killed
src/lib.rs:13:8 negate-condition killed
src/lib.rs:13:19 or-to-and killed
src/lib.rs:14:16 true-to-false killed
src/lib.rs:16:5 false-to-true killed
```
