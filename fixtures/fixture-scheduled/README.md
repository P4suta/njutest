<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-scheduled

A test that passes only on schedules where a thread it spawns answers within 50 ms.

| Function | Reached on | What a delayed schedule does |
| --- | --- | --- |
| `work` | a thread the test spawns | a pause of 200 ms at its guard makes the answer late, and the test fails |
| `unreached` | nothing | a pause there is never taken, and the test passes |

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
