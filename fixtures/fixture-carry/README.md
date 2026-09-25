<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-carry

A comparison whose operands come from inputs that are not Rust sources of the crate: `src/answer.txt`, read with `include_str!`, and a limit `build.rs` writes into `OUT_DIR` and the library reads with `include!`.
An edit to either changes what the compiled program computes, so no answer the outcome store kept before the edit may be read back after it.

## Fates

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:16:5 return-true survived
src/lib.rs:16:16 gt-to-ge survived
```
