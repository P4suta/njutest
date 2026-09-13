<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0001 — Test seams are arguments, not package-level variables

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0001 (accepted 2026-09-01).
Enforced from the first commit by `cargo xtask devgates` and the ledger
`xtask/seam_allowlist.txt`, which starts empty.

## Context

goatest tested the parts of itself that touch the filesystem, the process
table, and the toolchain by replacing the operation under test through a
package-level variable — `var readCacheFile = os.ReadFile` — that production
read, a test overwrote, and a cleanup restored. It was the smallest edit that
made a failure reachable, and 102 of them accumulated across eleven packages.
Three costs followed: no test in such a package could run in parallel; the
coupling was invisible in every signature; and the restore was the test's own
far-away obligation, so a forgotten one leaked a fault into the next test.

Rust makes the variable harder to write — a `static` needs interior
mutability, a `thread_local!` needs a macro — but not impossible, and the
same shape appears as `#[cfg(test)]` branches in production code, as
`std::env::var` reads scattered below the command line, and as
`std::process::exit` calls that end a process from inside a library. Each
hides a decision from the options that should carry it.

This repository starts empty, so it does not inherit the debt. What it
inherits is the rule and the gate.

## Decision

Behaviour a test replaces travels as an argument. Concretely:

1. **The exported API does not change.** A public function delegates to a
   private `xxx_with_hooks(args, hooks)` that takes the operations it performs.
2. **The hooks are one immutable struct of function fields per concern.** It
   is a value, passed by value, `Default` is production, and a test fills in
   only the operation it drives.
3. **Collaborators with state keep their traits.** `CommandWorkspace`,
   `MutationSession`, `Notes` are trait objects answered by the testkit; the
   run's whole table of collaborators is `assure::Dependencies`, a struct of
   boxed closures passed to the run rather than read from anywhere.
4. **Only the composition root reads the process.** `main.rs` alone reads the
   environment, the arguments, and the streams, names the executable, the
   user cache directory, and the temporary directory, and exits the process.
   Every layer below is configured by options.
5. **Test support is never imported by production code.** The devkit is a
   dev-dependency; a crate's `testkit` module is behind `cfg(test)` or the
   `testkit` feature.
6. **The rule is a ratchet.** `cargo xtask devgates` parses every production
   file with `syn` and compares what it finds — `static mut`, a `static` with
   interior mutability, `thread_local!`, `#[cfg(test)]` outside a `mod tests`,
   an environment read or an exit outside `main.rs`, a testkit import — with
   the ledger. The scan and the ledger must agree exactly, in both directions.
7. **The ledger grows only by amending this record.** A pull request may add a
   line only when one commit carries the seam, its ledger line, and an entry
   under [Exceptions](#exceptions) naming why the behaviour cannot travel as
   an argument and when the seam goes.

## Consequences

- Every crate's tests run in parallel from the start, and nothing has to be
  restored between tests.
- Fault-injection tests are longer and more explicit: they name the internal
  call they drive and build the hooks at the call. That is the price of the
  injection point being visible in the signature.
- The gate reads shape, not use: a `static` of an atomic is counted whether or
  not a test touches it. A generated runtime (the mutant runtime rust-mutants
  appends to instrumented files) is not this repository's source and is not
  scanned.
- The gate cannot tell an exception from an ordinary addition; only the
  amendment decision 7 requires can, which is why it goes in front of a
  reviewer.

## Exceptions

None. The ledger is empty.
