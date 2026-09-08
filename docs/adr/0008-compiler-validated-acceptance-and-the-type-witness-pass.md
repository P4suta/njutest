<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0008 — Compiler-validated acceptance and the type witness pass

## Status

Accepted, 2026-09-05. Implemented by the `validate` module of rust-mutants
(M1) and its witness tree (M5).

## Context

Every engine in the family reads the compiler's own evidence to decide what a
mutation may do: ocaml-mutants reads the Typedtree a normal Dune build leaves
behind, go-mutants asks `go/types`. Stable Rust exports no typed AST. The
choices were `rust-analyzer` as a library — a large, fast-moving dependency
whose answers are not guaranteed to match `rustc` — `rustc_private` on
nightly, or asking the compiler directly.

The engine already asks the compiler directly whether a candidate compiles: every
candidate is instrumented behind a guard, the tree is built once, and a
candidate the build rejects is bisected out and reported as `compile-rejected`
rather than as a survivor.

## Decision

1. **Discovery is syntactic; acceptance is the compiler's.** `syn` finds
   candidates. The instrumented tree is checked with
   `cargo check --message-format=json`; each error's primary span is
   attributed to the alternative branch it falls in, the mutant that owns it
   is condemned with the diagnostic, the file is re-instrumented from pristine
   without it, and the loop repeats until green. A file the loop cannot settle
   in six rounds, and an error that falls in no branch, go to delta-debugging
   bisection, which is always sound. A pristine tree that is already red stops
   everything: the failure is not mutant-induced.
2. **Return-value probes need no type spelling.** Rust's inference unifies
   `let __rm_r0 = E;` with the function's return type, and a trait bound with
   autoref specialisation lets the compiler refuse every probe the premise does
   not carry. A probe reads `==` as the answer to whether a test could have
   seen the replacement, so the sealed `Observable` names the types whose
   equality is the whole of what a program can tell apart — the integers,
   `bool`, `char`, the unit — and a float, or a type whose `PartialEq` answers
   about less than a test can read, is refused: the probe tree's own validation
   drops the site, and the mutant is simply unprobed.
3. **A proof that needs a fact about a type asks the compiler for it.** The
   branch proof needs a comparison that runs none of the program's code. A
   third tree — pristine plus one witness statement per candidate, checked and
   never run — gets `__rm::w_ord(&a, &b)` in front of the `if`; the sealed
   trait behind it names the types whose comparison is the language's or the
   library's rather than the program's — the primitives, `str`, and a slice of
   one of those — and a witness that fails to check leaves its candidate
   without a proof. A user type is refused because its `PartialOrd` is the
   program, and `String` because naming it would need `alloc`, which a
   `#![no_std]` crate the module is generated into may not have.
4. **`rust-analyzer` stays out.** A `TypeOracle` trait marks the extension
   point; nothing implements it.

## Consequences

- The target project adds no dependency, no feature, and no nightly. Stable
  `cargo` is the whole toolchain.
- A green tree costs one `cargo check`; a red one costs a few. The trace
  records every round and which diagnostic condemned which mutant, so a slow
  validation can be read.
- Type-directed operator families are impossible without types, so the
  operator table splits by syntax, not by type: `add-to-sub` covers integers
  and floats alike and the compiler rejects what does not type-check.
- Whether a fact was proved is always visible: `Mutant.branch` and
  `Mutant.probed` say what the compiler agreed to, and their absence says only
  that nothing was claimed.
