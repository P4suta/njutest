<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-balanced-fails-then-hangs

The same shape as [fixture-fails-then-hangs](../fixture-fails-then-hangs/README.md), asked about `ready(0)` so that the mutations of the `balanced` tier, the only one njutest measures, are the ones it notices.
`a_says_nothing_is_not_ready` fails at once under a mutation that calls nothing ready, and `b_waits_while_nothing_is_ready` then parks forever without running any of the library again, so nothing counts its steps and only the clock would end the process.
A failing test is the whole answer about a mutation, so the run ends the process there and concludes `killed` rather than waiting for the clock.
`crates/njutest/tests/toolchain_verify.rs` holds njutest's mutation phase to that.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `ready`: the comparison and its return |

```fates
src/lib.rs:9:5 return-true killed
src/lib.rs:9:11 gt-to-ge killed
src/lib.rs:9:13 int-increment survived
```
