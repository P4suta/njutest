<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-uncompiled

One library crate whose `src/elsewhere.rs` is a module no build compiles: `lib.rs` declares it under `#[cfg(any())]`, which holds on no platform, the way a module gated to another platform is absent from this one.
Discovery walks only the files a unit read, so `elsewhere.rs` holds no candidate, and a claim written about it is judged by [ADR 0042](../../docs/adr/0042-a-claim-holds-where-its-facts-do.md) rather than left unmatched.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `max`: its comparison and returns |
| `src/elsewhere.rs` | none | never cataloged; `seven` would be a `return-default` of `7` |

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:8 condition-to-false killed
src/lib.rs:11:8 condition-to-true killed
src/lib.rs:11:8 negate-condition killed
src/lib.rs:11:10 gt-to-ge not_run
src/lib.rs:11:16 return-default killed
src/lib.rs:11:27 return-default killed
```
