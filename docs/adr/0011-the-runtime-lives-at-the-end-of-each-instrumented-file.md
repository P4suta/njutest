<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0011 — The runtime lives at the end of each instrumented file

## Status

Accepted, 2026-09-05. Implemented by the `instrument` module of rust-mutants
(M1).

## Context

go-mutants generates one runtime package per module and imports it from
every guarded file with a fresh alias. Rust has no package-level import: a
module must be declared by its parent, which means finding the crate root of
every file, editing `lib.rs` or `main.rs`, and shifting the line numbers of
whatever follows the declaration. Line numbers are load-bearing — coverage
regions and mutant positions are compared by line and column — and the
instrumentation preserves them byte for byte.

## Decision

Each instrumented file gets a private `mod __rm { … }` appended after its last
line. The module holds the file's own mutant IDs and their dense indices, the
catalog digest, and `active(index) -> bool` backed by a racy-initialised
`AtomicU32` that reads `RUST_MUTANTS_ACTIVE` once. Guards inside nested inline
modules reach it with `super::`. The identifier is bumped (`__rm1`, …) if the
file already binds it. The module is `#[doc(hidden)]` and allows every lint,
so `#![deny(missing_docs)]`, `#![forbid(unsafe_code)]`, and clippy in the
target crate see nothing to object to.

The runtime compares the catalog digest it was generated with against
`RUST_MUTANTS_CATALOG` and exits with status 97 on a mismatch, so a stale
tree can never be mistaken for a survivor. The probe runtime is the same
module in the probe tree, appending to the infection log named by
`RUST_MUTANTS_PROBE`, and exits 98 when it cannot.

## Consequences

- No crate root is touched, no line moves, and a file reached through
  `#[path]` or compiled into both a library and a binary is correct without
  special handling.
- A file included at expression position by `include!` cannot carry the
  module; validation rejects that file's mutants rather than letting the
  breakage hide.
- `#![no_std]` crates are skipped as a whole with the reason `no-std-crate`:
  the runtime needs `std::env` and `std::process`.
