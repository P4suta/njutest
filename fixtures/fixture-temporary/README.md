<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-temporary

A library that keeps a scratch file where `TMPDIR` names, or in the standard library's temporary directory, and one integration test for each.

A sealed instance is given a temporary directory of its own, empty and written only to its overlay, and `TMPDIR` names it, so the test that keeps its file where `TMPDIR` names passes its control sealed, and nothing it made outlives the instance.
The standard library of `wasm32-wasip1` has no temporary directory: `std::env::temp_dir` panics in its platform layer whatever `TMPDIR` names.
So the test that keeps its file there has no sealed control, and the run names why: it met a refusal of the sandbox, `refused`.
The mutations that make the first test call `std::env::temp_dir` too, negating or falsifying `named` or emptying the name `TMPDIR`, pass natively, where the standard library has a temporary directory.
Sealed, the same panic stops them, and a failure the sandbox caused is no detection: each is unproven, with `refused` among its reasons, where before the sealed panic killed it.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default unproven
src/lib.rs:11:8 condition-to-false unproven
src/lib.rs:11:8 condition-to-true unproven
src/lib.rs:11:8 negate-condition unproven
src/lib.rs:12:9 return-default survived
src/lib.rs:12:26 string-to-empty unproven
src/lib.rs:14:9 return-default unproven
src/lib.rs:24:5 delete-call-statement killed
src/lib.rs:24:32 ignore-question-statement unproven
src/lib.rs:24:32 question-to-unwrap unproven
src/lib.rs:25:46 question-to-unwrap unproven
src/lib.rs:26:5 delete-call-statement unproven
src/lib.rs:26:32 ignore-question-statement unproven
src/lib.rs:26:32 question-to-unwrap unproven
src/lib.rs:27:5 return-ok-default killed
src/lib.rs:33:5 return-default killed
src/lib.rs:33:12 add-to-sub killed
src/lib.rs:33:14 int-decrement killed
src/lib.rs:33:14 int-increment killed
```
