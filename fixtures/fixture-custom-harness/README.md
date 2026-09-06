<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-custom-harness

A test target that says what it found by exiting.

`[[test]] harness = false` means there is no libtest: the program prints what
it likes and its exit status is the whole answer. Reading it for a
`test result:` line finds none, and a run that took that silence for "nothing
ran" left every mutation of the library undecided, so a project that tests
this way could not be measured at all.

The harness flag is in the metadata and the target carries it now. A target
without one is `custom-harness` in the limitations: a run cannot say how many
of its tests ran, only whether it was happy.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-true killed
src/lib.rs:9:7 gt-to-ge killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
```
