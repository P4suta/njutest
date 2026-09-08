<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-macros

A library that leans on macros, and a proc-macro member it depends on.

| Path | Candidates | Skips |
| --- | --- | --- |
| `src/lib.rs` | 3 (`return-default` on `double!(x + 1)`, `add-to-sub` and `return-default` in `g`) | `macro-invocation` 2 (`double!`, `println!`) |
| `crates/derive/src/lib.rs` | 6 | none |

A proc-macro crate's `--test` build is an ordinary executable that links the
crate as a library and runs its unit tests in a process of its own, so the
helpers it expands with are measured like anything else: `repeats` is killed by
`repeats_is_capped_at_three`. What is not measured is the expansion. `noop`
runs inside the compiler during the build, no test process ever activates a
mutant of it, and cargo does not rebuild for an environment variable — so its
mutants are reported as surviving and `proc-macro-expansion-not-measured` says
why. One of them used to be reported as killed, by the documentation target of
the library that reads the macro: it has no documented examples, so nothing
ran, and what failed was the build of the library under a macro that now
expands to something else. A target that answers nothing is left out of the
route, which puts the fates back where this paragraph says they are. The binary is built with `prefer-dynamic`, which is why the engine puts
the toolchain's library directories on the search path before starting a test
binary itself.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
crates/derive/src/lib.rs:11:5 return-default unreached
crates/derive/src/lib.rs:11:8 negate-condition unreached
crates/derive/src/lib.rs:11:16 int-decrement unreached
crates/derive/src/lib.rs:11:16 int-increment unreached
crates/derive/src/lib.rs:11:19 gt-to-ge unreached
crates/derive/src/lib.rs:11:21 int-decrement unreached
crates/derive/src/lib.rs:11:21 int-increment unreached
crates/derive/src/lib.rs:14:9 return-default unreached
crates/derive/src/lib.rs:20:5 return-default killed
crates/derive/src/lib.rs:20:8 negate-condition killed
crates/derive/src/lib.rs:20:10 gt-to-ge not_run
crates/derive/src/lib.rs:20:12 int-decrement survived
crates/derive/src/lib.rs:20:12 int-increment survived
crates/derive/src/lib.rs:20:16 int-decrement killed
crates/derive/src/lib.rs:20:16 int-increment killed
crates/derive/src/lib.rs:20:16 return-default killed
crates/derive/src/lib.rs:20:27 return-default killed
src/lib.rs:21:5 return-default unreached
src/lib.rs:27:5 return-default unreached
src/lib.rs:27:13 add-to-sub unreached
src/lib.rs:27:15 int-decrement unreached
src/lib.rs:27:15 int-increment unreached
```
