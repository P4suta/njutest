<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-macros

A library that leans on macros, and a proc-macro member it depends on.

| Path | Candidates | Skips |
| --- | --- | --- |
| `src/lib.rs` | 3 (`return-default` on `double!(x + 1)`, `add-to-sub` and `return-default` in `g`) | `macro-invocation` 2 (`double!`, `println!`) |
| `crates/derive/src/lib.rs` | 0 | `proc-macro-crate` 4 (the candidates it would have had) |
