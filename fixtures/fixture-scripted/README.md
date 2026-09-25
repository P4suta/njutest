<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-scripted

A build script reads `answer.txt`, says it depends on that file alone with `rerun-if-changed`, and puts what it read in the compiler's environment as `FIXTURE_ANSWER`.
`answer()` returns it through `env!`; `other()` returns one.
`tests/answer.rs` checks the first and `tests/other.rs` the second.

The compiler read `FIXTURE_ANSWER`, which no environment a selection runs under holds: the build script decides it.
A selection holds the script to what it said it watches, which is cargo's own rule for when to run it again, rather than to the environment.
An edit to `other` therefore skips `answer`, and an edit to `answer.txt` runs everything.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `other`: its literal |

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:15:5 int-decrement killed
src/lib.rs:15:5 int-increment killed
src/lib.rs:15:5 return-default killed
```
