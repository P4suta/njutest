<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0009 — `soundness` replaces `race`

## Status

Accepted, 2026-09-05. Implemented by the `soundness` phase of `assure` and
the `unsafe_scan` of `cargo` (M4), and by the Miri runner of `deep-v1` (M7).

## Context

goatest's race phase runs Go's race detector, because a data race in safe Go
is an ordinary bug the type system does not prevent. Safe Rust prevents data
races at compile time; the residual fault class is the unsoundness of
`unsafe` code — undefined behaviour inside `unsafe` blocks, `unsafe fn`,
`unsafe impl`, foreign functions, and `static mut`. Rust has no race
detector; it has Miri, an interpreter that detects undefined behaviour and
data races in the code it can run, and ThreadSanitizer on nightly.

## Decision

The phase is `soundness`, and the report's accounting field is
`accounting.soundness`.

- **`standard-v1`** takes a static inventory of the `unsafe` surface of every
  workspace crate and reports it as evidence. A crate with a non-empty
  inventory adds the limitation `soundness-not-executed` (estimated). The
  verdict may still be `ASSURED`, exactly as goatest's `standard-v1` is
  assured under `race-scope-static-estimate`: a fault outside the fault model
  is a limitation, not missing evidence. Safe-code data races are not a
  finding class because the compiler already refuses them.
- **`deep-v1`** runs `cargo +nightly miri test` on every crate with a
  non-empty inventory and every crate that links one into a test binary.
  Undefined behaviour is an `undefined-behaviour` finding (`DEFECT`); a test
  failure under Miri is `soundness-test-failure` (`DEFECT`); an operation
  Miri does not support is the limitation `miri-unsupported` and makes the
  verdict `INSUFFICIENT`. Miri absent under `deep-v1` is `ERROR`.
- ThreadSanitizer is a `deep-v1` option (`[soundness] sanitizers = ["thread"]`).

## Consequences

- A project with one `unsafe` block stays assurable on stable; the limitation
  says what was not executed.
- The inventory is fail-closed: a source file that does not parse counts as
  unsafe.
- The contract text names the fault model precisely, so `ASSURED` never reads
  as a soundness claim about `unsafe` code under `standard-v1`.
