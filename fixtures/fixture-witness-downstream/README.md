<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-witness-downstream

Two members, `upstream` and `downstream`, where `downstream` depends on `upstream`.
Each returns, by name, a type that has a `Default` and no equality a probe may trust, so each `return-default` compiles and neither may be probed.

The witness check is one `cargo check` of a tree holding every probe, and the compiler refuses `upstream`'s probe there.
A crate that fails stops cargo before the crates that depend on it, so that check never compiles `downstream` at all.
A check that never compiled a probe has not vouched for it.
It used to be read as having vouched: `downstream`'s probe went into the instrumented tree, validation could name no mutant for its error, bisected, and refused `downstream`'s `return-default` with the probe's words, while `upstream`'s, the same shape, was measured and killed.

The witness check now runs again without what it refused until one check compiles every target, and vouches for only what that check held.
Both `return-default`s are measured, and validation never bisects.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
crates/downstream/src/lib.rs:17:18 add-to-sub killed
crates/downstream/src/lib.rs:19:5 return-default killed
crates/upstream/src/lib.rs:16:34 int-decrement killed
crates/upstream/src/lib.rs:16:34 int-increment killed
crates/upstream/src/lib.rs:17:5 return-default killed
```
