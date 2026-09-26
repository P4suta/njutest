<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-bare-cargo

One library whose integration test runs `cargo -V` by its bare name, from the directory the test runs in, as a test that builds something or reads a manifest does.

A run measures a copy of the tree, and a shim that chooses a toolchain by the directory it runs in, as mise does, may refuse the copy: mise refuses a `mise.toml` it was never asked to trust, and the copy's is one.
The engine asks a bare `cargo` once, from the copy, with the environment the tests get.
Where it does not answer as the run's toolchain does, every test is given that toolchain's own directory first on its search path, so the shim is never asked.
`toolchain_cli_contract`'s `a_test_that_runs_a_bare_cargo_gets_the_runs_toolchain` runs this fixture behind a shim that refuses every directory but the fixture's own.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:7 mul-to-div killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
```
