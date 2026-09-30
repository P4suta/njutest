<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-temporary

A library that keeps a scratch file where `TMPDIR` names, or in the standard library's temporary directory, and one integration test for each.

A sealed instance is given a temporary directory of its own, empty and written only to its overlay, and `TMPDIR` names it, so the test that keeps its file where `TMPDIR` names passes its control sealed, and nothing it made outlives the instance.
The standard library of `wasm32-wasip1` has no temporary directory of its own: its `std::env::temp_dir` panics in its platform layer whatever `TMPDIR` names.
The sealed build links every module with an object whose function reads `TMPDIR` as the standard library of a POSIX system does, and rewrites the standard library's `std::env::temp_dir` to answer through it, so the test that keeps its file there passes its control sealed too.
The mutations that send the first test to `std::env::temp_dir` instead, negating or falsifying `named` or emptying the name `TMPDIR`, reach the same directory both ways, natively and sealed, so each survives, as a sealed verdict.
Before the rewrite, the platform layer's panic stopped them sealed: first it killed them, which the native run contradicts, and then, read as the sandbox's refusal, it left them unproven.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default survived
src/lib.rs:11:8 condition-to-false survived
src/lib.rs:11:8 condition-to-true survived
src/lib.rs:11:8 negate-condition survived
src/lib.rs:12:9 return-default survived
src/lib.rs:12:26 string-to-empty survived
src/lib.rs:14:9 return-default survived
src/lib.rs:24:5 delete-call-statement killed
src/lib.rs:24:32 ignore-question-statement survived
src/lib.rs:24:32 question-to-unwrap survived
src/lib.rs:25:46 question-to-unwrap survived
src/lib.rs:26:5 delete-call-statement survived
src/lib.rs:26:32 ignore-question-statement survived
src/lib.rs:26:32 question-to-unwrap survived
src/lib.rs:27:5 return-ok-default killed
src/lib.rs:33:5 return-default killed
src/lib.rs:33:12 add-to-sub killed
src/lib.rs:33:14 int-decrement killed
src/lib.rs:33:14 int-increment killed
```
