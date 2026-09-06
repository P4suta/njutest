<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-workspace

Two members under `crates/`: a library (`fixture-core`, with a submodule in
`src/util.rs`) and a binary (`fixture-app`) that depends on it and is run
by its own integration test through `CARGO_BIN_EXE_fixture-app`. It fixes
how the engine resolves the paths cargo and rustc report for a member that
is not at the workspace root, and how a test environment reproduces what
cargo would have set.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
crates/app/src/main.rs:12:8 negate-condition killed
crates/app/src/main.rs:12:16 ge-to-gt survived
crates/app/src/main.rs:13:9 delete-call-statement survived
crates/core/src/lib.rs:10:5 return-default killed
crates/core/src/lib.rs:10:8 negate-condition killed
crates/core/src/lib.rs:10:10 lt-to-le survived
crates/core/src/lib.rs:12:15 negate-condition killed
crates/core/src/lib.rs:12:17 gt-to-ge survived
crates/core/src/lib.rs:21:5 return-default killed
crates/core/src/util.rs:9:9 delete-compound-assignment killed
crates/core/src/util.rs:9:13 add-assign-to-sub-assign killed
crates/core/src/util.rs:11:5 return-default killed
```
