<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-integration-bodies

Two functions, `total` and `over`, each reached by its own test and never by the other's, as in `fixture-two-bodies`, with the tests in an integration test, `tests/apart.rs`, rather than beside the functions.
No mutation is in that file, so what records an execution entering a test's body there is the entry marker a run plants in every file a test program compiles, not the guards of a mutation.
A line added inside the last test is therefore an edit no execution of a mutant of `total` entered: every answer about `total` carries across it, and every answer about `over` is established again (ADR 0041).

## Fates

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:10 add-to-sub killed
src/lib.rs:13:5 return-true killed
src/lib.rs:13:11 gt-to-ge killed
src/lib.rs:13:13 int-decrement killed
src/lib.rs:13:13 int-increment killed
```
