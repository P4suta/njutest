<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-no-std

A `#![no_std]` library. The v1 runtime module needs `std` for the
environment lookup and the process exit, so every file of the crate is a
`no-std-crate` skip; `src/lib.rs` would have yielded 2 candidates
(`add-to-sub`, `return-default`).
