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

Seven mutants, five killed and two nothing could have noticed — and the two are the point.

| Mutant | Rule | Fate |
| --- | --- | --- |
| `src/lib.rs:8:5` | `return-default@1` | killed by `tests::sign_names_both_sides_of_zero` |
| `src/lib.rs:8:8` | `negate-condition@1` | killed by the same test |
| `src/lib.rs:8:10` | `gt-to-ge@1` | **discharged**: only `sign(0)` tells `>` from `>=`, so the guard's two branches never parted in the run that verified the baseline, and a run that started it would have found it survives |
| `src/lib.rs:10:15` | `negate-condition@1` | killed by the same test |
| `src/lib.rs:10:17` | `lt-to-le@1` | **discharged**: for the same reason, on the other side of zero |
| `src/lib.rs:19:5` | `return-default@1` | killed by `doubling::doubling_is_addition_twice` |
| `src/lib.rs:19:7` | `mul-to-div@1` | killed by the same test |

The two are the gap `#[ignore]` left, and no amount of green in the suite
would have shown it. That is what the phase is for. Neither is *run*: the
guards of the instrumented tree evaluated both branches at each condition on
the baseline and never saw them part, so the run reports them as discharged
mutations nothing could have noticed rather than as survivors it measured
([ADR 0015](../../docs/adr/0015-the-guard-is-the-infection-probe.md)). A
finding either way, and one fewer process.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge not_run
src/lib.rs:8:12 int-increment killed
src/lib.rs:9:9 return-default killed
src/lib.rs:9:9 string-to-empty killed
src/lib.rs:10:15 negate-condition killed
src/lib.rs:10:17 lt-to-le not_run
src/lib.rs:10:19 int-increment survived
src/lib.rs:11:9 return-default killed
src/lib.rs:11:9 string-to-empty killed
src/lib.rs:13:9 return-default unreached
src/lib.rs:13:9 string-to-empty unreached
src/lib.rs:19:5 return-default killed
src/lib.rs:19:7 mul-to-div killed
src/lib.rs:19:9 int-decrement killed
src/lib.rs:19:9 int-increment killed
```
