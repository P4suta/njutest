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
