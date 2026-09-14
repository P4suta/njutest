<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-assured

The workspace a run can honestly call `ASSURED`: every mutation the compiler
accepts is noticed by a test.

| Mutant | Rule | Killed by | Why it dies |
| --- | --- | --- | --- |
| `src/lib.rs:9:5` | `return-true@1` | `tests::is_positive_is_false_at_zero_and_below` | the test asserts a `false`, which a function that always returns `true` cannot produce |
| `src/lib.rs:9:7` | `gt-to-ge@1` | the same test | `is_positive(0)` is the boundary the change moves |
| `src/lib.rs:15:5` | `return-default@1` | `doubling::doubling_four_is_eight` | `0` is not `8` |
| `src/lib.rs:15:7` | `mul-to-div@1` | the same test | `4 / 2` is not `8` |

Four mutants, four kills, no acceptance. The tests are written for the
boundaries rather than for the happy path, which is the whole difference
between this fixture and `fixture-baseline`.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-true killed
src/lib.rs:9:7 gt-to-ge killed
src/lib.rs:9:9 int-increment killed
src/lib.rs:15:5 return-default killed
src/lib.rs:15:7 mul-to-div killed
src/lib.rs:15:9 int-decrement killed
src/lib.rs:15:9 int-increment killed
```
