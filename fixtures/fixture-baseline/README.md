<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-baseline

What the runner's baseline phase is measured against: two libtest binaries,
three tests, and two functions that no single test reaches together.

| Target | Fate | Why it is here |
| --- | --- | --- |
| `lib` `tests::sign_names_both_sides_of_zero` | passes | reaches two of `sign`'s three regions and none of `double` |
| `lib` `tests::zero_has_a_sign_of_its_own` | skipped | `#[ignore]`: a baseline reports it as skipped and never as a pass it did not observe |
| `test` `doubling::doubling_is_addition_twice` | passes | reaches `double` and none of `sign` |

The two passing targets reach disjoint regions on purpose: coverage routing
is only worth anything if per-target coverage differs, and a baseline that
merged them into one set would hide that it does not.

## What the mutation phase finds

Seven mutants, five killed and two survived — and the two are the point.

| Mutant | Rule | Fate |
| --- | --- | --- |
| `src/lib.rs:8:5` | `return-default@1` | killed by `tests::sign_names_both_sides_of_zero` |
| `src/lib.rs:8:8` | `negate-condition@1` | killed by the same test |
| `src/lib.rs:8:10` | `gt-to-ge@1` | **survives**: only `sign(0)` tells `>` from `>=`, and the test that passes zero is the ignored one |
| `src/lib.rs:10:15` | `negate-condition@1` | killed by the same test |
| `src/lib.rs:10:17` | `lt-to-le@1` | **survives**: for the same reason, on the other side of zero |
| `src/lib.rs:19:5` | `return-default@1` | killed by `doubling::doubling_is_addition_twice` |
| `src/lib.rs:19:7` | `mul-to-div@1` | killed by the same test |

The two survivors are the gap `#[ignore]` left, and no amount of green in
the suite would have shown it. That is what the phase is for.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge survived
src/lib.rs:10:15 negate-condition killed
src/lib.rs:10:17 lt-to-le survived
src/lib.rs:19:5 return-default killed
src/lib.rs:19:7 mul-to-div killed
```
