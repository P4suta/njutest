<!--
SPDX-FileCopyrightText: 2026 njutest contributors
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

Each instrumented file gets a private `mod __rm_<digest> { … }` appended after
its last line, where the digest is the first eight hex characters of the file
path's SHA-256. The path is in the name because `include!` at item position
pastes one file's items into another file's module: two files that both called
their module `__rm` would define one item twice in one scope, the compiler
would say so in an error that names no mutant, and every mutant of both files
would come back `compile-rejected` — the engine's own breakage reported as the
compiler's judgement about the user's code. The module holds the file's own mutant IDs and their dense indices, the
catalog digest, and `active(index) -> bool` backed by a racy-initialised
`AtomicU32` that reads `RUST_MUTANTS_ACTIVE` once. Guards inside nested inline
modules reach it with `super::`. The identifier is bumped (`__rm1`, …) if the
file already binds it. The module is `#[doc(hidden)]` and allows every lint,
so `#![deny(missing_docs)]`, `#![forbid(unsafe_code)]`, and clippy in the
target crate see nothing to object to.

The runtime compares the catalog digest it was generated with against
`RUST_MUTANTS_CATALOG` and exits with status 97 on a mismatch, so a stale
tree can never be mistaken for a survivor.

The same module is also what measures reach. When `RUST_MUTANTS_TOUCH` names
a file, every guard records the thread that reached it, and libtest names
each test's thread after the test, so the record is per test rather than per
target. It buffers per thread and appends on the thread's way out; a thread
nothing can name a test after — the main one, one a test spawned — writes
`-`, and everything under that name has to reach every test of its target.
The process exits 96 rather than running on when it cannot record, because
silence is what licenses a run to skip a test.

## Consequences

- No crate root is touched, no line moves, and a file reached through
  `#[path]` or compiled into both a library and a binary is correct without
  special handling.
- Every file carries its own copy of the module, so what the module holds is
  paid for once per instrumented file. Sixty copies of it compile in 0.74 s
  and 209 MB against 0.18 s and 119 MB for the same sixty without the
  recording, which is about 9 ms and 1.5 MB a file — measured with `rustc
  --crate-type lib` on one generated file holding all sixty. That is what a
  shared runtime crate would buy, and it is not worth the crate boundary this
  decision exists to avoid.
- A file included at expression position by `include!` cannot carry the
  module, and does not parse as a set of items either: it is a fragment of one
  program rather than a program. Discovery reads the includes of the files that
  do parse, and skips such a file with the reason `included-expression` rather
  than stopping the run over a project the compiler is happy with.
- A skip reason may be added to `rust-mutants-catalog-v1` without a new schema
  version. A reader that does not know a reason knows it is a place that was
  not mutated, which is what the field is for; a reader that must understand
  every reason is reading the explanation beside it, which is prose.
- The runtime borrows `std` under a name of its own — `extern crate std as
  __rm_std` inside the generated module — so a `#![no_std]` crate whose tests
  run on the host is measured like any other. `#![no_std]` withholds the
  implicit link and the prelude; it forbids neither an explicit link nor an
  explicit path, and no `unsafe` is written or needed. What stays a
  `no-std-crate` skip is a crate the host cannot lend `std` to: one with a
  `#[panic_handler]` or a `#[global_allocator]` of its own, or `#![no_main]`,
  each of which `std` also supplies and only one of which may exist. Edition
  2015 stays skipped because `extern crate` resolves differently there.
