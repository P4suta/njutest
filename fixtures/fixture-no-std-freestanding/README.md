<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-no-std-freestanding

A `#![no_std]` library that supplies its own `#[panic_handler]`. `std`
supplies one too, and only one may exist, so this is a crate the host cannot
lend `std` to and the runtime module has nowhere to read the environment
from.

Every file of the crate is a `no-std-crate` skip; `src/lib.rs` would have
yielded 2 candidates (`add-to-sub`, `return-default`).

This fixture exists so that lifting the skip from `fixture-no-std` cannot
quietly lift it from here as well.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
```
