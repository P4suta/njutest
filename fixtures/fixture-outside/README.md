<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-outside

A mutation that stops a loop ending without being in it, or in its file.

`count_to` writes `let step = 1` and hands it to `spin::walk`, whose `while` loop adds it until a total is reached.
Mutating that literal to `0` is what makes the loop never end, and the mutation is neither in the loop nor in the file the loop is in.
`spin.rs` has mutations of its own, but none of them is this one.

That shape is why this fixture exists.
The count reaches it anyway, because checkpoints are placed in every mutable file — including one with no mutant of its own — and a checkpoint charges the allowance once the selected mutation has been reached.
So the loop body counts on the literal's behalf, the allowance is crossed, and the fate below is `step_limit_reached` rather than `waited`.

`docs/limitations.md` said the opposite until this was measured: that a guard sits only where its mutation does, so a mutation outside a loop was taken once and could be stopped by nothing but the clock.
It described the instrumenter before checkpoints were placed across a whole workspace.
Nothing in the tree repeated the measurement, which is how a page kept saying it; the fates block below is what repeats it now, and it fails if that stops being true.

What is still the clock's is code the instrumenter does not rewrite — macro expansions and dependency crates — and this fixture says nothing about that.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`; `cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:16 int-decrement step_limit_reached
src/lib.rs:11:16 int-increment killed
src/lib.rs:12:5 return-default killed
src/spin.rs:9:18 int-increment survived
src/spin.rs:10:21 int-increment killed
src/spin.rs:11:11 negate-loop-condition killed
src/spin.rs:11:14 lt-to-le killed
src/spin.rs:12:9 delete-compound-assignment killed
src/spin.rs:12:15 add-assign-to-sub-assign killed
src/spin.rs:13:9 delete-compound-assignment step_limit_reached
src/spin.rs:13:12 add-assign-to-sub-assign killed
src/spin.rs:15:5 return-default killed
```
