<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-silent-kill

`tests/limit.rs` starts the `child` binary with a cleared environment and holds `limit()` to what the child says.
The child runs unmutated and records nothing it enters, since the variables that ask it to are gone.
Every mutant of `limit` is killed, by an execution a process ran inside without saying what it entered, so the union that execution records is not the whole of what it entered: every record a run keeps of it says `cut`, and no answer about it is carried across an edit (ADR 0041, P3).

```fates
src/lib.rs:8:5 int-decrement killed
src/lib.rs:8:5 int-increment killed
src/lib.rs:8:5 return-default killed
```
