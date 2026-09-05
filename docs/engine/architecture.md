<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# rust-mutants architecture

**Status: designed, not implemented.** The library crate exists with its
error ledger; everything below arrives in M1 in the order of the
[roadmap](../roadmap.md).

## Invariants

1. **The source workspace is read-only.** `Workspace::open` copies the tree
   into a disposable snapshot under the temporary root, at a name stable per
   repository root so successive runs share cargo's incremental state. Every
   build, instrumentation, and test happens in the copy. Symbolic links,
   devices, and other irregular files are refused, not skipped: a skipped
   link is a silently absent file.
2. **Instrumentation happens once.** Every compilable mutant of a file lives
   dormant behind a guard in the snapshot; the test binaries are built once;
   `RUST_MUTANTS_ACTIVE=<64 hex id>` activates one mutant per test process.
3. **Bytes are spliced, never pretty-printed.** Comments, whitespace, and CRLF
   are preserved, and every splice keeps its line count, so coverage regions
   and mutant positions agree line for line with the pristine file.
4. **Phases are types.** `Workspace::open → Workspace::prepare(self) → Session`;
   a session executes any number of (mutant, target) pairs without rebuilding.

## Pipeline

```text
cargo metadata (in the snapshot) ─→ pristine cargo check + dep-info
        │
   syntactic discovery (syn): candidates, site hints, skips with reasons
        │
   instrument all candidates ─→ cargo check ─→ attribute errors to branches
        │                              └─ re-instrument without them, repeat; bisect what does not settle
   verify (cargo test, no mutant active)
        │
   cargo test --no-run ─→ test binaries, outside the snapshot
        │
   [probe tree: second snapshot, probe runtime, infection log]
   [witness tree: third snapshot, checked only, branch proofs]
        │
   Session: exec(mutant, target, args) / probe(target, args) / changes()
```

## Guards

Three forms. **Form C** for a position that is syntactically boolean (an
`if` or `while` condition, an operand of `&&`/`||`, a match guard):
`__rm::active(3) && (a >= b) || !(__rm::active(3)) && (a > b)`. **Form E**
for any expression in value position, in parentheses:
`(if __rm::active(5) { a - b } else { a + b })` — both branches unify to one
type, so `Default::default()` is inferred from the original. **Form S** for
a statement: `if __rm::active(7) { x -= step; } else { x += step; }`, the
original bytes in the `else` so lines are kept. A position where wrapping
would move a value out of place — an assignment target, a borrow operand, a
method receiver, a scrutinee — escalates to its parent expression, then to
the statement.

The runtime is a private `mod __rm` appended after the last line of each
instrumented file ([ADR 0011](../adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)).
A guarded function gets `#[allow(warnings)]` on the same line as its first
token, so the warnings guards provoke never trip a `#![deny(warnings)]`.

## Skips, stated

`const-context`, `macro-invocation`, `cfg-attribute`, `test-code`,
`no-std-crate`, `proc-macro-crate`, `test-only-file`. Each is counted and
named; `rust-mutants why-skipped` lists them. A skip is a decision the tool
made and says; a rejection (a mutant the compiler refused) is a fact about
the program and is reported with the diagnostic.

## Execution

A test binary is started directly, never through `cargo test`, with the
environment cargo would give it, `RUST_MUTANTS_*` stripped and set, a
scratch `TMPDIR`, and the libtest arguments verbatim. The outer supervisor
owns the timeout and the process tree; exit status is read in this order —
start failure → `errored`; timed out → `timed_out`; killed by us → `not_run`;
97 → `errored` (stale catalog, never a kill); non-zero → `killed`; zero →
`survived`. A libtest run that matched no test is green and says so:
`tests_run` carries the count the summary line reported.

## Identity

```text
id = SHA-256( enc("rust-mutants-id-v1") ‖ enc(path) ‖ enc(rule) ‖ enc(rule version)
              ‖ enc(start byte) ‖ enc(end byte) ‖ enc(source sha256) ‖ enc(original sha256)
              ‖ enc(replacement sha256) )
enc(s) = 4-byte big-endian length ‖ UTF-8 bytes
```

The same recipe as go-mutants with this domain string, so an identity is
reproducible from the catalog fields alone in any language. `display_id` is
the first twenty hex digits; a prefix of four or more resolves a mutant.

## Trace

Every decision the engine takes — every site's form and skip reason, every
validation round and the diagnostic that condemned each mutant, every bisect
step, every build and execution — is recorded to the sink `OpenOptions`
names. See [trace](trace.md).
