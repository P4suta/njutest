<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-scheduled

A test that passes only on schedules where a thread it spawns does its work within 90 ms.

| Function | Reached on | What a delayed schedule does |
| --- | --- | --- |
| `work` | a thread the test spawns | a pause at its guard (100 ms from njutest's explorer, 200 ms from the engine's own test) makes the work late, and the test fails |
| `unreached` | nothing | a pause there is never taken, and the test passes |

The spawned thread measures its own work and sends that duration with the answer, and the test waits 150 ms before it reads it.
Lateness is measured where the pause is taken, never by how long the test thread waited: a waiter that a busy machine takes off the processor for longer than the pause begins waiting after a late answer has already arrived, and a deadline counted from the wait then calls it on time.
That is how a loaded macOS runner reported a delayed schedule as passing, and the 150 ms wait makes every run of this fixture that machine.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:7 add-to-sub killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
src/lib.rs:15:5 return-default unreached
src/lib.rs:15:7 add-to-sub unreached
src/lib.rs:15:9 int-decrement unreached
src/lib.rs:15:9 int-increment unreached
```
