<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-targets

The target kinds a run has to tell apart, and a path it has to carry.

- **An example cargo also tests.** `[[example]] test = true` makes `demo` a
  target with its own tests, and the mutations of `whole` are killed by it and
  by nothing else. An engine that only looked at `lib` and `tests/` would
  report both as surviving.
- **A benchmark with no harness.** `[[bench]] harness = false` builds
  `throughput` and no run executes it. It is something the build produces that
  is not a place to look for an answer, which is the distinction the target
  list has to make.
- **Edition 2021.** Every other fixture is edition 2024. The instrumentation,
  the guards, and the runtime module have to compile under both.
- **A module in a directory whose name is not ASCII.** `src/ünits/` is
  reached through a `#[path]` attribute, because rustc refuses to look for a
  file named by a non-ASCII identifier on its own. The snapshot has to copy
  it, the report has to name it, and the identity has to hash a path that is
  not one byte per character.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:12:5 return-default killed
src/lib.rs:12:7 div-to-mul killed
src/ünits/mod.rs:12:5 return-true unreached
src/ünits/mod.rs:12:7 ge-to-gt unreached
```
