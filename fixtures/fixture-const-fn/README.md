<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-const-fn

Three uses of a `const fn`, which decide whether its body is mutated ([ADR 0047](../../docs/adr/0047-a-const-fn-is-mutated-where-nothing-evaluates-it-early.md)).
A `const fn` holding a guard is written without its `const` in the instrumented tree, so the guard's call at run time compiles.
Where the compiler evaluates the function before the program runs, it refuses that tree with `E0015` at the call, and validation gives the function its `const` back by leaving out every mutant it holds.

| Function | Used by | Fate |
| --- | --- | --- |
| `below_ten` | a test, while the program runs | mutated: `return-true`, `lt-to-le` and `int-increment` killed, `int-decrement` (`n < 9`) survives |
| `double` | `const DOUBLED`, before the program runs | every mutant `evaluated-before-run`, found in the first round |
| `twice_successor` | `static CHAINED`, before the program runs | every mutant `evaluated-before-run`, found in the first round |
| `successor` | `twice_successor`, once the static has given it back its `const` | every mutant `evaluated-before-run`, found in the second round |

The survivor is the finding the fixture keeps: the tests ask about zero and ten and never about nine, so they cannot tell "below ten" from "below nine".
The chain is the point of the last two rows.
The first round's refusal names `twice_successor` and says nothing of `successor`, which a caller written without its `const` may call; only once `twice_successor` is `const` again does the compiler refuse the call inside it, and since the static evaluates the caller, the callee keeps its `const` too.
Each round here gives at least one function its `const` back, so the rounds end, and the build is never refused.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate validation left out.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:0:0 add-to-sub evaluated-before-run
src/lib.rs:0:0 int-decrement evaluated-before-run
src/lib.rs:0:0 int-decrement evaluated-before-run
src/lib.rs:0:0 int-decrement evaluated-before-run
src/lib.rs:0:0 int-increment evaluated-before-run
src/lib.rs:0:0 int-increment evaluated-before-run
src/lib.rs:0:0 int-increment evaluated-before-run
src/lib.rs:0:0 mul-to-div evaluated-before-run
src/lib.rs:0:0 mul-to-div evaluated-before-run
src/lib.rs:0:0 return-default evaluated-before-run
src/lib.rs:0:0 return-default evaluated-before-run
src/lib.rs:0:0 return-default evaluated-before-run
src/lib.rs:9:5 return-true killed
src/lib.rs:9:7 lt-to-le killed
src/lib.rs:9:9 int-decrement survived
src/lib.rs:9:9 int-increment killed
```
