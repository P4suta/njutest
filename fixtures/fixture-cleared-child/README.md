<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-cleared-child

One function, `answer`, that only a child process runs.
The integration test `cleared` starts the `child` binary with `env_clear()` and checks what it printed.

A cleared environment is one the run's variables do not reach: no mutant can be active in the child, and nothing records what it entered.
Before the run looked for such processes, every mutation of `answer` was reported `unreached`, a claim that no test reached it, when the one test that ran it had simply never let a mutant in.

The run now sees the child say it lost the environment, names `cleared` with `uncontrolled-child`, keeps it in every route, and reads a survival it reports as inconclusive rather than as a survivor.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `answer`: its return and its arithmetic |

```fates
src/lib.rs:8:5 int-decrement inconclusive
src/lib.rs:8:5 int-increment inconclusive
src/lib.rs:8:5 return-default inconclusive
src/lib.rs:8:8 add-to-sub inconclusive
src/lib.rs:8:10 int-decrement inconclusive
src/lib.rs:8:10 int-increment inconclusive
```
