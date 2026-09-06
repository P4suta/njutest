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
- A mutant every test routing reads carries coverage about and none of them
  reaches is reported as surviving, with a detail that says nothing executes
  it. Where that evidence is not there — the position is outside every
  instrumented region, the catalog could not place it, or a target that was
  measured for its coverage carries none — the package suite runs and settles
  it instead. Both readings are one finding, because they are one gap in the
  suite.
- A test that writes into the tree while it is being measured makes every
  later mutation a measurement of what it wrote
  (`tree-written-during-measurement`). One instrumented snapshot cannot
  isolate that the way a per-mutant build would; the run says so rather than
  reporting the later results as if it had.

## Decided in advance

- Doctests are run as one target per library, without coverage, and route no
  mutant (`doctests-not-routed`). A mutant only a doctest could kill is
  reported as surviving, and the `unreached` reading of that says which tests
  it is about.
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
- Mutation inside a macro invocation the engine has no allowlist entry for, a
  `const` context, and `#[cfg]`-guarded code is skipped with a stated reason.
  The leading arguments of the six assertion macros are mutated; the rest of
  every invocation is `macro-invocation`.
- A `#![no_std]` crate is measured: the runtime borrows `std` under a name of
  its own. A crate the host cannot lend `std` to — one supplying a
  `#[panic_handler]`, a `#[global_allocator]`, or `#![no_main]`, or written in
  edition 2015 — is `no-std-crate`.
- A file another file pastes in with `include!` where an expression goes is
  `included-expression`: it is a fragment rather than a program.
- The equivalence layer is off by default and proves almost nothing on a
  project that leaves `[profile.test] opt-level` at cargo's default of zero,
  where two mutations the compiler would render identically at any
  optimisation level are still two different sets of instructions. It is a
  fact about the profile the tests run under rather than about the mutation,
  and the layer answers the question the tests ask rather than an easier one.
- A procedural macro's own unit tests are measured like any others; what it
  expands to is not (`proc-macro-expansion-not-measured`). A macro decides that
  during the build and a mutation is activated for a test process, so the two
  never meet, and cargo does not rebuild for an environment variable. A
  mutation only the expansion would change is reported as surviving.
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
