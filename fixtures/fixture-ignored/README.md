<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-ignored

A library whose every test carries `#[ignore]`, so its one target runs
nothing.

A target that runs no test decides nothing, and the engine calls that
`inconclusive` rather than passed or failed. What it does not decide by itself
is *why* nothing ran: a harness that skipped every test and a harness that
printed no summary at all both run none. `Baseline::ignored` is what tells them
apart, and this fixture is what says so when it stops.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `double`: `return-default`, `mul-to-div`, `int-increment`, `int-decrement` |

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default unreached
src/lib.rs:9:7 mul-to-div unreached
src/lib.rs:9:9 int-decrement unreached
src/lib.rs:9:9 int-increment unreached
```
