<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-two-bodies

Two functions, `total` and `over`, each reached by its own test and never by the other's.
An edit inside the body of `total` is therefore an edit no execution of a mutant of `over` entered, which is what ADR 0041 carries an answer across: after such an edit, every answer about `over` carries from the earlier run, and every answer about `total` is established again.

## Fates

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:10 add-to-sub killed
src/lib.rs:13:5 return-true killed
src/lib.rs:13:11 gt-to-ge killed
src/lib.rs:13:13 int-decrement killed
src/lib.rs:13:13 int-increment killed
```
