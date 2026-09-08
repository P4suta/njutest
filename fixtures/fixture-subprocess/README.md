<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-subprocess

A library nothing calls in the test process. The one test runs the package's
own binary, which calls the library, so every region of `decide` is executed
by a process the test started rather than by the test itself.

| Function | Reached | Fate |
| --- | --- | --- |
| `decide` | through the binary | `negate-condition` and every `return` replacement are killed by `the_binary_names_both_sides_of_zero`; `gt-to-ge` is discharged, nothing having seen its two branches part |

This fixture exists because coverage is read out of the profiles a target's
processes wrote, and a profile is only readable against the binary that wrote
it. A run that reads a target's profiles against that target's own executable
alone loses everything its children did, and then reports code the tests do
execute as code nothing reaches.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge not_run
src/lib.rs:8:12 int-increment killed
src/lib.rs:8:16 return-default killed
src/lib.rs:8:16 string-to-empty killed
src/lib.rs:8:36 return-default killed
src/lib.rs:8:36 string-to-empty killed
src/main.rs:7:37 int-decrement killed
src/main.rs:7:37 int-increment killed
```
