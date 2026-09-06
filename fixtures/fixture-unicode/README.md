<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-unicode

Non-ASCII identifiers and comments before the code that matters, so the
question "which column?" has one answer that a test can hold both tools to.

`llvm-cov` region columns are 1-based **byte** columns; rustc's *diagnostic*
columns are **character** columns. One toolchain uses both units, so neither
is a safe default, and the engine records both for every mutant. On the line
of `大きい方`, the comparison's two columns differ by twelve, so a test that
compared a coverage region against the wrong one fails here.

`使われない` is called by nothing: its region is instrumented and uncovered,
which is what "no test reaches this" looks like in a coverage export.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:7:61 return-default killed
src/lib.rs:7:64 negate-condition killed
src/lib.rs:7:67 gt-to-ge survived
src/lib.rs:11:5 return-default unreached
src/lib.rs:11:8 mul-to-div unreached
```
