<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-hang

A mutation that never returns, and a test that can be slow exactly once.

`count_to` sums every step below `n` in a `while` loop. Deleting `step += 1`
leaves the loop with nothing that ends it, and no test can notice a function
that does not return: the run has to stop it, run it once more on its own, and
report `timed_out` only when it happens again. Nothing else in the suite has a
mutation like that, so the bound on one execution, the serial retry, and the
process tree the runner kills were all carried by tests that never reached
them.

`clamp_positive` is ordinary, and the test of it sleeps once per activation
when `FIXTURE_HANG_MARKER` names a directory and `FIXTURE_HANG_PAUSE_MS` says
how long. That is the other half of the contract: a timeout that does not
reproduce is `inconclusive` rather than `timed_out`, because a loaded machine
is a different machine from the one a bound was calibrated on. The fates below
are the ones without the marker set, so the fixture's ordinary run stays
ordinary; `crates/rust-mutants-cli/tests/toolchain_hang.rs` sets it.

`.rust-mutants.toml` puts the bound at two seconds. A mutation that never
returns costs the bound twice, and a suite has to be able to afford waiting
for it.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:20 int-increment survived
src/lib.rs:10:21 int-increment killed
src/lib.rs:11:11 negate-loop-condition killed
src/lib.rs:11:16 lt-to-le killed
src/lib.rs:12:9 delete-compound-assignment killed
src/lib.rs:12:15 add-assign-to-sub-assign killed
src/lib.rs:13:9 delete-compound-assignment timed_out
src/lib.rs:13:14 add-assign-to-sub-assign killed
src/lib.rs:13:17 int-decrement timed_out
src/lib.rs:13:17 int-increment killed
src/lib.rs:15:5 return-default killed
src/lib.rs:25:5 return-default killed
src/lib.rs:25:8 negate-condition killed
src/lib.rs:25:10 gt-to-ge survived
src/lib.rs:25:12 int-increment survived
src/lib.rs:25:16 return-default killed
src/lib.rs:25:27 int-increment killed
```
