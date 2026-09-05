<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Current limitations and deferred work

**Status: scaffold.** The list grows with each milestone; every limitation
below is stated fail-closed.

- Nothing below the command line is implemented yet. `mjutest` and
  `rust-mutants` parse their arguments, print their help, and exit.

## Decided in advance

- Doctests are run as one target per library, without coverage, and route no
  mutant (`doctests-not-routed`). A mutant only a doctest could kill is
  reported as surviving.
- A `harness = false` test target is one target per binary
  (`custom-harness-whole-binary`).
- cargo-fuzz targets are discovered from v1 but executed only from M7; until
  then `fuzz-not-executed` is a limitation, and without a nightly toolchain
  it stays one.
- `standard-v1` does not execute anything about `unsafe` code; it
  inventories it and says so (`soundness-not-executed`). `deep-v1` needs Miri.
- Repository reads are not observed at run time. A package that uses a
  directory-reading API keys the whole snapshot for evidence reuse; nothing
  is excluded from testing.
- Mutation inside macro invocations, `const` contexts, `#[cfg]`-guarded code,
  and `#![no_std]` crates is skipped with a stated reason.
- One workspace per run. Path dependencies outside the workspace root are
  refused unless explicitly allowed as read-only.
- Symbolic links in the evidence tree are rejected.
