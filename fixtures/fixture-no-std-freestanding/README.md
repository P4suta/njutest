<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
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
