<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-writes-tree

A test that writes into the tree it is being measured in.

A run takes one instrumented snapshot and measures every mutation against it,
which is what makes it fast and what makes this a problem: a test that writes
a file into the snapshot has changed what every later mutation is measured
against. A per-mutant build would not have the problem and would cost a build
per mutant, so the engine keeps the snapshot and says when one was written to
instead of reporting the later results as though nothing had happened.

`Session::changes` is what says it. This fixture exists so that saying it is
tested against a tree that really is written to, rather than against a tree
nobody wrote to and an assertion that the list is empty.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:7 add-to-sub killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
```
