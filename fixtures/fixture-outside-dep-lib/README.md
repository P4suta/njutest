<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-outside-dep-lib

The library `fixture-outside-dep` reads from beside itself. It is a fixture of
its own so that the path climbing out of that one climbs into something real,
and it is never measured on its own: nothing mutates a dependency it only
reads.

## Fates

A run of this fixture on its own reaches every mutation and decides none of
them: the library has no tests, so nothing speaks and the run says so rather
than calling silence a survivor. What it is for is measured through
`fixture-outside-dep`, which reads it.

```fates
src/lib.rs:9:5 return-default unreached
src/lib.rs:9:7 mul-to-div unreached
```
