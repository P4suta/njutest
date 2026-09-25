<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-carry

A comparison whose operands come from inputs that are not Rust sources of the crate: `src/answer.txt`, read with `include_str!`, and a limit `build.rs` writes into `OUT_DIR` and the library reads with `include!`.
`build.rs` also emits `cargo::rustc-cfg=waived` when the package holds a `waive` file, which waives the limit through `cfg!(waived)`.
An edit to any of these, or a `waive` file appearing, changes what the compiled program computes, so no answer the outcome store kept before it may be read back after it.

## Fates

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:19:5 return-true survived
src/lib.rs:19:16 gt-to-ge survived
src/lib.rs:19:24 or-to-and killed
```
