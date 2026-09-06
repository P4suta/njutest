<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
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

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:21 add-to-sub killed
```
