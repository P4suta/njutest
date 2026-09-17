<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-order-dependent

A test that only passes beside its neighbour.

Routing a mutation to the tests that reached it is sound only where running
those tests on their own asks the same question as running the target. Here it
does not: `beside` waits for `prepares` to have run, so a process that runs
`beside` alone fails for a reason that is about the pair rather than about any
mutation. A run that took that failure for a kill would report a mutation as
noticed when nothing noticed it, which is the one direction a proof layer may
never go.

So the run establishes the premise before it uses it. The first time a set of
tests is named as a filter, it is put once with nothing active. This one does
not pass, the run notes `test-routing-unsound`, and every mutation of this
target is put to every test of it from then on. The note is in the recording,
and the fates below are exactly the fates a run with nothing removed gives.

| Function | Reached by | What routing does |
| --- | --- | --- |
| `larger` | `prepares` | the set does not answer on its own, so the whole target runs |
| `next` | `beside` | the same |

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:21:5 return-default killed
src/lib.rs:21:8 condition-to-false survived
src/lib.rs:21:8 condition-to-true killed
src/lib.rs:21:8 negate-condition killed
src/lib.rs:21:10 gt-to-ge not_run
src/lib.rs:21:16 return-default unreached
src/lib.rs:21:27 return-default killed
src/lib.rs:27:5 return-default killed
src/lib.rs:27:7 add-to-sub killed
src/lib.rs:27:9 int-decrement killed
src/lib.rs:27:9 int-increment killed
```
