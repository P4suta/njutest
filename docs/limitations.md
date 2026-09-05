<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Current limitations and deferred work

**Status: implemented.** Every name here is one a report can carry; the list grows with each release, and every limitation
below is stated fail-closed.

- A tree whose identity could not be computed reuses nothing
  (`workspace-digest-not-computed`) and is reused by nothing: a run that
  cannot say what it looked at cannot answer for another run's inputs.
- A mutant no measured test reaches is reported as surviving, with a detail
  that says which of "nothing reached it" and "nothing noticed it" it is.
  They are one finding because they are one gap in the suite.
- A test that writes into the tree while it is being measured makes every
  later mutation a measurement of what it wrote
  (`tree-written-during-measurement`). One instrumented snapshot cannot
  isolate that the way a per-mutant build would; the run says so rather than
  reporting the later results as if it had.

## Decided in advance

- Doctests are run as one target per library, without coverage, and route no
  mutant (`doctests-not-routed`). A mutant only a doctest could kill is
  reported as surviving.
- A `harness = false` test target is one target per binary
  (`custom-harness-whole-binary`).
- Fuzz targets are found always and driven only when `[fuzz] run` says so;
  a tree that holds targets nobody asked to drive carries
  `fuzz-not-executed`. Without cargo-fuzz on a nightly toolchain, a run that
  was asked to drive them carries `cargo-fuzz-unavailable` and a
  `not-measured` finding instead.
- `standard-v1` does not execute anything about `unsafe` code; it
  inventories it and says so (`soundness-not-executed`). `deep-v1` interprets
  the suite under Miri and refuses to run at all without it (`MJ7001`), and
  what Miri will not interpret is `miri-unsupported` rather than a pass. A
  sanitizer the configuration asks for and the toolchain will not run is
  `sanitizer-unavailable`; every sanitizer run also carries
  `sanitizer-standard-library-not-instrumented`.
- Repository reads are not observed at run time. A package that uses a
  directory-reading API keys the whole snapshot for evidence reuse; nothing
  is excluded from testing.
- Mutation inside macro invocations, `const` contexts, `#[cfg]`-guarded code,
  and `#![no_std]` crates is skipped with a stated reason.
- Coverage routing says nothing about a place its export never instrumented,
  and such a mutant is run everywhere. A tree that configures its own
  `rustflags` is not routed by coverage at all
  (`coverage-refused-configured-rustflags`), because a coverage build would
  have to replace them.
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
