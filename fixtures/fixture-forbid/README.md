<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-forbid

Two members that treat the same lint differently. `forbids` puts
`#![forbid(unused_qualifications)]` at its crate root; `denies` sets the same
lint to `deny` in its manifest.

| Path | Unit | Candidates |
| --- | --- | --- |
| `forbids/src/lib.rs` | lib and test | skipped whole: `forbidden-lints` |
| `denies/src/lib.rs` | lib and test | `min`: `lt-to-le`, `negate-condition`, `return-default` |

`forbid` is the one lint level an `allow` cannot override, and the attribute
every guard carries is an `allow`. A guard placed in `forbids` would be a
compile error whatever it edited, so every mutant of that crate would be
refused and nothing in the report would say why. The crate is skipped whole
and named instead. `deny` is exactly what the guards' attribute is carried
for, so `denies` is measured like any other crate.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
denies/src/lib.rs:8:5 return-default killed
denies/src/lib.rs:8:8 negate-condition killed
denies/src/lib.rs:8:10 lt-to-le not_run
denies/src/lib.rs:8:16 return-default killed
denies/src/lib.rs:8:27 return-default unreached
```
