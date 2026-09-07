<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-threaded

A test that reaches the code on a thread of its own.

The guards record the name of the thread that reached them, and libtest names
each test's thread after the test. A thread the test spawned has no name a test
answers for, so what it reached is recorded as `-`: a touch nothing can be
attributed to. Everything under that name reaches **every** test of its target,
because the measurement could not say which one, and the answer to not knowing
is to run more rather than fewer.

| Function | Reached on | What routing does |
| --- | --- | --- |
| `larger` | a thread the test spawned | recorded loose, so every test of the target runs |
| `next` | the test's own thread | put to `the_test_itself_reaches_the_next` alone |

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:8 negate-condition killed
src/lib.rs:9:10 gt-to-ge survived
src/lib.rs:9:16 return-default unreached
src/lib.rs:9:27 return-default killed
src/lib.rs:15:5 return-default killed
src/lib.rs:15:7 add-to-sub killed
src/lib.rs:15:9 int-decrement killed
src/lib.rs:15:9 int-increment killed
```
