<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-hang

A mutation that never returns, and a test that can be slow exactly once.

`count_to` sums every step below `n` in a `while` loop. Deleting `step += 1`
leaves the loop with nothing that ends it, and no test can notice a function
that does not return. The guard of the active mutant sits where the mutation
does, so it is taken once an iteration, and the count of takes ends the
process where a clock used to: the mutation is `step_limit_reached`, an exact
execution boundary but not a detection, and it is **not** retried as a clock
measurement. Nothing else in the suite has a mutation like
that, so the step allowance, the status the runtime leaves by, and the
process tree the runner kills are carried by tests that reach them nowhere
else.

`clamp_positive` is ordinary, and the test of it sleeps once per activation
when `FIXTURE_HANG_MARKER` names a directory and `FIXTURE_HANG_PAUSE_MS` says
how long. That is the other half of the contract, and since a count took over
the first half it is the only one that reaches the clock at all: a bound that
expires once and not again is `inconclusive` rather than `waited`, because a
loaded machine is a different machine from the one a bound was calibrated on.
The serial retry is carried here now, and no longer by `count_to`, which ends
at the allowance and is never asked twice. The fates below
are the ones without the marker set, so the fixture's ordinary run stays
ordinary; `crates/rust-mutants-cli/tests/toolchain_hang.rs` sets it.

`walked` is the mutation outside the loop it stops ending.
Its stride is the mutation's site, and the loop that walks it is in `src/walk.rs`, which a `rust-mutants: skip` marker keeps the run from mutating.
Decrementing the stride to zero never ends that loop, and the checkpoints the instrumenter places across every mutable file, this one included, count it: the mutation is `step_limit_reached` at exactly one past the allowance and never `waited`, which `toolchain_hang.rs` holds.

The two halves used to want `.rust-mutants.toml`'s bound pulled in opposite directions, and no longer do.
`clamp_positive` needs the bound low enough that a sleep of a few seconds outlasts it, or nothing times out and there is no `inconclusive` to observe.
`count_to` once needed it high enough that the count got there first, because the bound was on the whole execution and a slow enough machine let the clock stop a mutation the allowance was about to catch.
For an execution that counts, the bound is now how long it may go without raising the count, so a loop that keeps taking its guard is never stopped by it; only the ceiling of ten bounds could still race the allowance.
A sleep raises nothing, so it meets the bound exactly as it did.

`FIXTURE_HANG_STRIDE_MS` makes the clamping test slow and moving: while a mutation is active it passes through `clamp_positive` two hundred times, that many milliseconds apart.
With a stride of fifty and a bound of five seconds the test is slower than the bound and never quiet for one, and `toolchain_hang.rs` sets it to show the run waiting for it.
The window runs from the start of the process, so the bound also has to outlast starting it before the first step: a bound of one second lost that race on a loaded Windows runner.

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
src/lib.rs:13:9 delete-compound-assignment step_limit_reached
src/lib.rs:13:14 add-assign-to-sub-assign killed
src/lib.rs:13:17 int-decrement step_limit_reached
src/lib.rs:13:17 int-increment killed
src/lib.rs:15:5 return-default killed
src/lib.rs:25:5 return-default killed
src/lib.rs:25:8 condition-to-false killed
src/lib.rs:25:8 condition-to-true killed
src/lib.rs:25:8 negate-condition killed
src/lib.rs:25:10 gt-to-ge survived
src/lib.rs:25:12 int-increment survived
src/lib.rs:25:16 return-default killed
src/lib.rs:25:27 int-increment killed
src/lib.rs:33:18 int-decrement step_limit_reached
src/lib.rs:33:18 int-increment killed
src/lib.rs:34:5 return-default killed
```
