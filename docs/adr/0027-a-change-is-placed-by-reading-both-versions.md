<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0027 — A change is placed by reading both versions

## Status

Accepted, 2026-09-24.
Implemented by the `select` module of rust-mutants.
The measurement it reads is [ADR 0026](0026-an-item-is-entered-where-its-body-starts.md)'s item reach, and the standing of each target is [ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)'s drift.

## Context

`select` skips a target only where it can show the target's tests cannot notice a change: they entered none of the items the change is to, on a measured tree whose reach held on a second run.
The first core took the byte ranges a `git diff` names in the measured file and placed each in the innermost item body holding it.
A review ran two counterexamples against it and named eight more classes, and every one of them is a way that placing *old* ranges misses what the *new* text does:

- A line inserted between a signature and a `{` that opens its own line is an empty range at the body's first byte, which the body holds, yet it is a `where` or `-> T` every caller type-checks against.
- `impl Drop for Beta { … }` inserted into `alpha`'s body is placed in `alpha`, yet every target that drops a `Beta` now runs it without entering `alpha`.
  So are a `#[no_mangle]` fn, a `#[macro_export]` macro, and any macro that expands to one of them.
- An edit that adds a line moves every later item, and their code embeds where it is: a panic's location, `line!()`, `Location::caller()` through `#[track_caller]`.
  A test that snapshots an error carrying a location enters only the moved item and still fails.
- A hunk is what git says, and git does not see an ignored file that is compiled, or a measured file whose bytes are not the ones the measurement read.

## Decision

1. **Both versions are read.** The input for one file is its measured bytes and its bytes now, not a list of ranges.
   The measured bytes are accepted only through `Changed::read`, which compares their SHA-256 with the digest the measurement recorded; bytes that differ are `Everything::Unproven`, so a stale checkout or a filter cannot pass for the measured file.

2. **The skeleton must be one skeleton.** Each version is lexed, and the tokens strictly between the braces of every outermost measurable body — its *interior* — are set aside.
   What is left is the skeleton: every item, signature, attribute, `use`, type, `const`, `static` and `const fn` body.
   Two skeletons that are not the same tokens are `Everything::Skeleton`.
   A token touching a brace is in the skeleton by construction, so the first counterexample has nowhere to hide.
   New bodies are found by the same visitor that numbered the measured ones, so the two readings agree about what a body is.

3. **An interior is unchanged only in place.** Two interiors are the same only if they are the same tokens at the same lines and columns.
   A line added in one body therefore changes every later body of the file, which is exactly the set whose locations moved, and a comment that moves nothing changes nothing.
   A body the guards cannot record — a `const fn`, a `static`'s initializer — is compared in place as well, and one that moved is `Everything::Unmeasurable`, because nobody's entry says who ran it.

4. **A changed interior must not reach past itself.** A changed interior holding `impl`, an exporting attribute, or a macro invocation outside a closed set of standard expression macros is `Everything::Escapes`.
   A name is the standard one only where nothing in its package can supply another: `Shadows` reads every file of the package for a `macro_rules!` of that name, a `use` from outside the standard library that imports or renames to it, a glob from outside the package, and a `#[macro_use] extern crate`, which hides every name.
   The same holds for the `test` attribute and the standard derive names, which an import can shadow as well — `use tokio::test;`, `use derive_more::Debug;` — and a changed interior holding such a `use` is `Everything::Escapes` itself.
   The built-in attributes cannot be shadowed, which rustc refuses as ambiguous, so they are not read for.

5. **What a foreign macro reads is compared in place.** An item carrying an attribute outside the inert set, or a `derive` of anything but the standard traits, is fed to a macro whose expansion may carry the span of what it read.
   Its tokens are compared at their lines and columns, and one that moved is `Everything::Located`.

6. **A doc comment is rustdoc's.** Outside the items of point 5, a `///` or `//!` attribute is left out of the skeleton.
   The doc target is never measured, since its guards cannot record, so it always runs; nothing else reads the text.

7. **The decision is total over the targets that exist now.** `decide` takes the targets the changed tree holds, not the ones the measurement names, so a target added since is `Unmeasured` and runs, and a caller can only leave out what `Selection::skippable` names.

## Consequences

- An insertion near the top of a file changes every body below it.
  That is the cost of point 3, and it is the honest one: those bodies say where they are.
- A file that holds no measured item — data, a `tests/common` module read by `mod`, a file only `include_str!` reads — is `Everything::Unitemized` until the measurement records which units compiled or included it.
- What this module cannot see is what the measurement does not yet record, and it is the next change: which unit kinds compiled each file (a proc-macro or build-script unit runs inside the compiler), the digests of every dep-info input inside and outside the root, the build identity beyond the files (features, profile, triple, `RUSTFLAGS`, effective cargo configuration, the values behind `env-dep` lines), whether a target reads the tree as data at run time, and whether a child process with a cleared environment records at all.
  Until each is measured, the file names in `builds_everything` and the fallbacks above are what keep a selection sound, and nothing skips a build: a selection says which targets need not run, never which need not compile.
