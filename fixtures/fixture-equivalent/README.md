<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-equivalent

A library built at `opt-level = 2`, which is what makes the question
interesting: `x + 0` and `x - 0` are the same instructions there and different
ones at the `opt-level = 0` cargo gives a test profile by default.

| Function | Mutation | What the compiler does | What the run says |
| --- | --- | --- | --- |
| `unchanged` | `add-to-sub` (`n + 0` → `n - 0`) | renders it identically | `equivalent`: every test that reaches it did, and could not have told the two apart |
| `doubled` | `mul-to-div` (`n * 2` → `n / 2`) | renders it differently | `killed` by `what_the_tests_call` |
| `halved` | `div-to-mul` | renders it identically, because nothing calls `halved` and the linker drops it | **not** `equivalent`: no test reaches it, so the identical bytes say the code is untested rather than that the mutation is unobservable |

The last row is the whole reason this layer needs a premise the engine cannot
check. Identical artifacts mean "no test can tell these apart", which is
reassuring when the tests run the code and is the finding itself when they do
not.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:7 add-to-sub survived
src/lib.rs:13:5 return-default killed
src/lib.rs:13:7 mul-to-div killed
src/lib.rs:18:5 return-default survived
src/lib.rs:18:7 div-to-mul survived
```
