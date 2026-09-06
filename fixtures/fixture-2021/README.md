<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-2021

An edition 2021 crate that reaches the standard library the older way, with
`extern crate alloc` and a `use` of a path through it. Editions change how
paths resolve and how the generated runtime module has to name `std`, so a
crate on the edition before the current one is a crate the engine has to be
run against.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `total`: `delete-compound-assignment`, `add-assign-to-sub-assign`, `return-default`; `above`: `negate-condition`, `gt-to-ge`, `delete-call-statement`, `return-default` |

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:14:9 delete-compound-assignment killed
src/lib.rs:14:13 add-assign-to-sub-assign killed
src/lib.rs:16:5 return-default killed
src/lib.rs:23:12 negate-condition killed
src/lib.rs:23:17 gt-to-ge killed
src/lib.rs:24:13 delete-call-statement killed
src/lib.rs:27:5 return-default killed
```
