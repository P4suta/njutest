<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-apparatus

A sweep that a mutation turns on the directory the test binaries run from.
`sweep` removes the files in a directory whose names say they are stale, and the one test sweeps the directory its own binary runs from, where nothing is stale, so the baseline removes nothing.
Three mutations make it remove everything there — `negate-condition`, `condition-to-true`, and `string-to-empty` on `"stale-"` — and the test still passes, so the mutant survives while the run's test executables are gone.

Before RM5009 every mutant after such a one came back `errored` with exit -1, for a cause it did not have; domyjob's run of its own node module was 131 of 145 that way.
A run of this fixture now stops at the first of the three with `RM5009`, naming it, which `toolchain_apparatus` holds.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `sweep`: conditions, strings, compound assignment, return default |

## Fates

What one run of this fixture establishes for every mutation it can finish, which is every one but the three that remove the test binaries, left out by rule because a run that reaches them stops.
The run is `rust-mutants run --tier all --offline --locked --skip-rule negate-condition --skip-rule condition-to-true --skip-rule string-to-empty`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --skip-rule negate-condition --skip-rule condition-to-true --skip-rule string-to-empty
src/lib.rs:10:23 int-increment survived
src/lib.rs:12:12 condition-to-false survived
src/lib.rs:12:12 condition-to-true not_run
src/lib.rs:12:12 negate-condition not_run
src/lib.rs:12:60 string-to-empty not_run
src/lib.rs:13:13 delete-compound-assignment unreached
src/lib.rs:13:21 add-assign-to-sub-assign unreached
src/lib.rs:13:71 is-ok-to-is-err unreached
src/lib.rs:16:5 return-default not_run
```
