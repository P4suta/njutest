<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-no-std

A `#![no_std]` library whose tests run on the host. `#![no_std]` withholds
the implicit link to `std` and its prelude; it forbids neither an explicit
link nor an explicit path, so the runtime module declares
`extern crate std as __rm_std` and the crate is measured like any other. No
`unsafe` is written and none is needed.

| Function | Fate |
| --- | --- |
| `add` | `add-to-sub` and `return-default` are killed by `adding_is_adding` |
| `at_least` | `ge-to-gt` is killed by the boundary case and `return-true` by the case below it |

This fixture exists because the crate used to be skipped whole with the
reason `no-std-crate`, on the strength of an attribute that says less than
the skip did.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:10:5 return-default killed
src/lib.rs:10:7 add-to-sub killed
src/lib.rs:15:5 return-true killed
src/lib.rs:15:7 ge-to-gt killed
```
