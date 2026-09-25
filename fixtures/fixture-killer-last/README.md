<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-killer-last

`double` is reached by two targets: its own unit test, which asks only `double(0)` and so passes some mutations of it, and the integration test `tests/notices.rs`, which asks `double(3)` and notices them.
Targets are asked in name order, so the library's test comes first and the integration test last.
A run remembers which target killed each mutant, and the next run asks that one first, which is what `src/unrelated.rs` is edited for: it makes the outcome store miss without changing any mutant of `double`.

## Fates

```fates
src/lib.rs:12:5 return-default killed
src/lib.rs:12:7 mul-to-div killed
src/lib.rs:12:9 int-decrement killed
src/lib.rs:12:9 int-increment killed
```
