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

## Snapshot

`snapshot::create` copies the tree byte for byte into
`<dest>/rust-mutants-snap-<16 hex of sha256(absolute root)>/tree`. The
name is stable so cargo's fingerprints survive from one run to the next;
the directory beside `tree` carries the `tempowner` lock and marker, so
every byte under `tree` came from the source. A directory found under the
stable name is swept and copied into again, never adopted; a live, kept, or
young unowned one makes the run fall back to a fresh name (reported through
`Snapshot::stable_dir`). `.git` at any depth and `reports/mutation` are
always excluded; a symbolic link, reparse point, device, or backslash-named
entry is refused with the first offending path in sorted order.

The manifest is sorted by path and hashed under the domain
`rust-mutants-workspace-v1` as `enc(domain) ‖ enc(path) ‖ enc(sha256hex) …`
(4-byte big-endian length prefixes). `Snapshot::redigest` re-walks the copy
with no exclusions and reports every added, removed, or changed path: the
gate that catches a test writing into its own package. Cleanup releases the
lock, checks a guard (absolute, prefixed name, expected parent), and retries
the removal on a 20/40/80/160 ms ladder; `keep` records the decision in the
marker so the next sweep obeys it.

### The public API

`Workspace::open` sweeps the temporary area, copies the tree, and locates
the toolchain inside the copy. `Workspace::prepare` consumes the workspace
and returns a `Session`: the phases are types, so a mutant cannot be
executed against a tree that was never prepared. A session is `Send + Sync`
and every execution takes `&self`, so a consumer runs mutants in parallel
across the targets one build produced.

The engine reads no environment variable of its own. The environment every
command and test process runs with, and the temporary directory everything
is created in, are arguments ([ADR 0001](../adr/0001-seam-policy.md)); the
composition root is the only place that names the process environment, and
`cargo xtask devgates` refuses any other.

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

### What instrumentation writes

`rust_mutants::instrument` composes the guards and appends a `mod __rm` to
each rewritten file. Alternatives of one site share one chain, nested sites
become nested guards composed children first, and only the branch that keeps
the original carries the guards inside it. Each alternative is folded onto
one line and the original branch keeps its bytes, so a guard holds exactly
as many line breaks as the bytes it replaced and every byte stays on its
line. `#[allow(warnings)]` goes on the signature line of the innermost
function that holds a guard, so a crate's deny policy is untouched
elsewhere. The runtime reads `RUST_MUTANTS_ACTIVE` once per process; a
`RUST_MUTANTS_CATALOG` that is not the one the tree was built from ends the
process with exit 97, and an identity this file does not know activates
nothing here because it belongs to another file.

### How a candidate becomes a mutant

`rust_mutants::validate` compiles the instrumented tree and reads the
diagnostics. Every alternative occupies a known byte range, so an error
whose primary span falls inside one is about exactly that mutant: a whole
round's refusals are condemned at once, and the loop costs one
recompilation per round rather than one per candidate. An error that falls
outside every branch is isolated by bisection instead, and a tree that does
not compile with nothing live stops the run (`RM4001`) rather than blaming
candidates until the error goes away. What comes back is always a tree that
compiled, plus a rejection per refused candidate carrying the compiler's own
words.

Whether an edit is a program is a fact about the toolchain, never assumed:
`fixtures/fixture-rejectable` records that this compiler refuses
`String - &str` and a `RangeInclusive` where a `Range` belongs, and accepts
`value / 0` for a run-time `value` — a mutant that dies at run time rather
than at compile time.

## Skips, stated

`const-context`, `macro-invocation`, `cfg-attribute`, `test-code`,
`unsupported-site`, `excluded`, `test-only-file`, `proc-macro-crate`,
`no-std-crate`. Each is counted and named; `rust-mutants why-skipped` lists
them. A skip is a decision the tool made and says; a rejection (a mutant the
compiler refused) is a fact about the program and is reported with the
diagnostic.

The per-file walk (`rust_mutants::syntax`) keeps walking inside a region it
will not mutate and counts every candidate it would have produced under the
outermost reason, so the tallies say how much code each reason hides. A
macro invocation counts once, since its body is tokens the walker does not
parse. Whole-file reasons (`excluded`, `test-only-file`, `proc-macro-crate`,
`no-std-crate`) are decided by the workspace layer from cargo metadata and
dep-info, not by the walk.

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
