<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-build-script

A library that reads a file its own build script wrote.

`build.rs` writes `table.rs` into the build directory and says
`cargo::rustc-env=FIXTURE_BUILD_TAG=…`. `src/lib.rs` pastes the table in with
`include!(concat!(env!("OUT_DIR"), "/table.rs"))`, which is the shape every
project that generates code has.

Two things are being kept honest here.

**A file the build wrote is passed over by name.** It is not in the tree, a
reviewer does not edit it, and the next build writes over any change to it, so
it is `generated-outside-workspace` and the rest of the library is measured.
Before that it was `RM2005`, which ended the whole run.

**A test process is told where the build directory is.** Cargo tells a unit
that reads a build script where the script wrote, and a test reads it back
through `OUT_DIR`. The engine starts test processes itself, so it has to say
so too: `a_test_process_is_told_where_the_build_directory_is` fails otherwise,
for a reason that is not the mutation.

**A sealed instance holds the build directory too.** The sealed build runs the build script for its own target and gives the test the directory it wrote as `OUT_DIR`, which is outside the tree an instance is given.
The instance holds that directory as the build left it, at the same path, so the test reads `table.rs` there and passes its control sealed, where before it failed its control and had no sealed answer at all.
`toolchain_sealed_build`'s `a_test_that_reads_where_its_build_script_wrote_passes_its_control_sealed` holds it.

The generated source and opaque macro expansion keep `tag` const, with its one candidate counted as `unvalidated-const-use` ([ADR 0047](../../docs/adr/0047-a-const-fn-is-mutated-where-nothing-evaluates-it-early.md)).
The four candidates in `one_more_than_total` are measured and killed.

`FIXTURE_BUILD_SCRIPT_PAUSE_MS` makes the build script take that many
milliseconds, and `FIXTURE_BUILD_SCRIPT_MARKER` names a file it creates before
it waits, so a test about interrupting a run can wait until the run is
certainly inside a compilation. Unset, both do nothing and the fates below are
what a run establishes.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:21 add-to-sub killed
src/lib.rs:11:23 int-decrement killed
src/lib.rs:11:23 int-increment killed
```
