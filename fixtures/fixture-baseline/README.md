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
