<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-features

Two conversions the library always compiles, and a test for the second one
that is behind a feature nothing turns on by default. A run of the defaults
can notice a mutation of `metres` and cannot notice one of `feet`; a run given
`--features imperial` compiles the second test module and notices both.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `metres`: `return-default`, `div-to-mul`; `feet`: `return-default`, `div-to-mul` |

The mutated code is never behind a `#[cfg]`, because the walker skips what a
`#[cfg]` gates (`cfg-attribute`): a mutation in a branch the compiler removes
would never compile and never die. What a feature changes here is which tests
exist, which is the thing a run's `[build]` configuration decides.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:17 div-to-mul killed
src/lib.rs:13:5 return-default unreached
src/lib.rs:13:12 div-to-mul unreached
```
