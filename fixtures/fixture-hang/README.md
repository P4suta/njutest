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
process where a clock used to: the mutation is `runaway`, it counts as
detected, and it is **not** retried, because a count cannot disagree with
itself on a second reading. Nothing else in the suite has a mutation like
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

The two halves want `.rust-mutants.toml`'s bound pulled in opposite
directions. `clamp_positive` needs it low enough that a sleep of a few
seconds outlasts it, or nothing times out and there is no `inconclusive` to
observe. `count_to` needs it high enough that the count gets there first, or
the clock stops a mutation the allowance was about to catch and the fate
table becomes a fact about the machine that generated it — measured here at
two seconds under load, where fifty million takes cost 1501 ms and one of the
two non-terminating mutations came back `waited` while the other came back
`runaway`. They are the same kind of mutation; only the moment differed.

That pull is the reason the allowance is a separate setting from the bound,
and the reason to give this fixture a small one rather than a generous
bound: a sleep only grows under load, so the clock half is safe with a low
bound, and a count is the same number under any load, so the count half is
safe with a small allowance. Raising the bound to buy the count room takes it
away from the sleep.

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
src/lib.rs:25:8 condition-to-false killed
src/lib.rs:25:8 condition-to-true killed
src/lib.rs:25:8 negate-condition killed
src/lib.rs:25:10 gt-to-ge survived
src/lib.rs:25:12 int-increment survived
src/lib.rs:25:16 return-default killed
src/lib.rs:25:27 int-increment killed
```
