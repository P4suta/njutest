<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-verify-fails

A tree whose own test fails before anything is mutated.

Mutation testing asks whether a test that passes stops passing. A target that
was already failing answers that question with the same failure whatever is
active, so every mutation it touches would be reported as killed by a test
that never noticed anything. The run refuses instead, with `RM5002`, and says
which target failed.

`--no-verify` is the way past it, and the point of this fixture is that going
past it is a decision rather than a default: with `--no-verify` every mutation
of `double` is `killed`, and none of those kills is about the mutation.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked --no-verify`, because a run that verifies
refuses this tree rather than reaching a fate at all;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --no-verify
src/lib.rs:9:5 return-default killed
src/lib.rs:9:7 mul-to-div killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
```
