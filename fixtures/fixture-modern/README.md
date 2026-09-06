<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-modern

The shapes a crate written today is made of, each with a test that notices a
mutation of it: iterator chains with closures, `impl Trait` in argument
position, a generic function with a bound, a struct with an inherent `impl`,
a trait with a default method, an `Iterator` implementation, `?` in a
`Result` function, and `async fn` with `.await` — run by a `block_on` the
fixture writes itself, because a fixture has no dependencies.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | every function above; see the fates |

Every mutant the compiler takes here is killed. That is the point of it: a
shape the walker reads wrongly, or a test that does not cover what it looks
like it covers, shows up as a survivor rather than as silence somewhere else.

Nothing here is an `unsupported-site` and nothing here is refused: every place
the rules target is a place a guard can be written, and the return types the
syntax cannot say have a default — `T` with no bound, `F::Output` — are stated
as `unstated-return-type` rather than offered for the compiler to refuse.

Both assertions about the counter are bounded with `take`. A mutation that
stops the counter stopping has to fail rather than run for ever: an unbounded
`collect` under such a mutation is a fixture that eats the machine rather than
a fixture that states a fate.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:19:5 return-true killed
src/lib.rs:19:11 le-to-lt killed
src/lib.rs:24:5 return-ok-default killed
src/lib.rs:24:8 negate-condition killed
src/lib.rs:25:9 return-ok-default killed
src/lib.rs:27:9 return-ok-default killed
src/lib.rs:33:5 return-default killed
src/lib.rs:36:24 mul-to-div killed
src/lib.rs:42:5 return-default killed
src/lib.rs:42:5 return-some-default killed
src/lib.rs:42:39 remove-not killed
src/lib.rs:49:9 delete-compound-assignment killed
src/lib.rs:49:13 add-assign-to-sub-assign killed
src/lib.rs:51:5 return-default killed
src/lib.rs:58:9 delete-assignment killed
src/lib.rs:59:30 delete-match-arm killed
src/lib.rs:59:30 remove-match-guard killed
src/lib.rs:63:5 return-default killed
src/lib.rs:82:9 return-true killed
src/lib.rs:82:17 lt-to-le killed
src/lib.rs:90:12 negate-condition killed
src/lib.rs:90:12 remove-not killed
src/lib.rs:91:20 return-some-default killed
src/lib.rs:93:9 delete-compound-assignment killed
src/lib.rs:93:17 add-assign-to-sub-assign killed
src/lib.rs:94:9 return-default killed
src/lib.rs:94:9 return-some-default killed
src/lib.rs:102:9 return-default killed
src/lib.rs:111:9 return-default killed
src/lib.rs:117:5 return-ok-default killed
src/lib.rs:122:50 question-to-unwrap killed
src/lib.rs:123:38 add-to-sub killed
src/lib.rs:123:55 question-to-unwrap killed
src/lib.rs:124:5 return-ok-default killed
src/lib.rs:124:14 add-to-sub killed
```
