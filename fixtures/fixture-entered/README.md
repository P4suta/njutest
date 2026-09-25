<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-entered

A branch the tests never take, behind a condition a mutation can force.
`pick` answers `rare` for a large input and `common` for a small one, and the one test asks about a small one, so the baseline enters `pick` and `common` and never `rare`.
A mutation that forces `n > 10` true makes the process enter `rare`, and the union of items a mutant execution records has to say so: it is the case ADR 0041's carried answers rest on, an execution entering an item the baseline's sets never mention.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `common`, `rare`, `pick`: arithmetic, conditions and return defaults |

## Fates

What one run of this fixture establishes for every mutation of it.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:7 add-to-sub killed
src/lib.rs:8:9 int-decrement killed
src/lib.rs:8:9 int-increment killed
src/lib.rs:13:5 return-default unreached
src/lib.rs:13:7 mul-to-div unreached
src/lib.rs:13:9 int-decrement unreached
src/lib.rs:13:9 int-increment unreached
src/lib.rs:18:5 return-default killed
src/lib.rs:18:8 condition-to-false survived
src/lib.rs:18:8 condition-to-true survived
src/lib.rs:18:8 negate-condition survived
src/lib.rs:18:10 gt-to-ge not_run
src/lib.rs:18:12 int-decrement survived
src/lib.rs:18:12 int-increment survived
src/lib.rs:18:17 return-default unreached
src/lib.rs:18:34 return-default killed
```
