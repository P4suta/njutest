<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-target-tmpdir

A library whose only test writes a file into `CARGO_TARGET_TMPDIR`, the directory cargo gives an integration test to write in, and reads it back.

cargo bakes that directory into the test as the path the build spelled it at, inside the target directory, which is outside the tree a sealed instance is given.
A sealed instance holds it as a directory of its own, empty and writable, at that same path, so the test's control passes sealed and its write lands in the instance's overlay.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:12 add-to-sub killed
src/lib.rs:9:14 int-decrement killed
src/lib.rs:9:14 int-increment killed
```
