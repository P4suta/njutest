<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Current limitations and deferred work

**Status: scaffold.** The list grows with each milestone; every limitation
below is stated fail-closed.

- The engine (`rust-mutants`) works end to end. `mjutest verify` measures a
  baseline — every test once, on its own, under coverage instrumentation —
  and writes a report; the mutation phase arrives in M3, so every run states
  `mutation-phase-not-implemented` and no run reaches an assurance. A suite
  that passes says only that it passes: whether those tests would notice a
  change has not been asked yet, and a verdict that claimed otherwise would
  be the one thing this program exists not to do.
- The evidence identity of the tree is not computed
  (`workspace-digest-not-computed`), so nothing is reused between runs. It
  arrives in M4 with the cache.

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
- The coverage build sets `CARGO_ENCODED_RUSTFLAGS`, which replaces
  `build.rustflags` rather than adding to it, so mjutest reads the project's
  `.cargo/config.toml` files and puts those flags back. What it does not put
  back is `target.<triple>` and `target.cfg(…)` flags — which of them apply
  is cargo's decision about the target being built, and guessing wrong would
  compile something other than the project's binaries. A project that
  configures them gets `target-rustflags-not-merged`, and a configuration
  file that cannot be parsed gets `cargo-configuration-unreadable`.
