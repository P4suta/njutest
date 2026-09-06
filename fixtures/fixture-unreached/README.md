<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-unreached

A mutation no measured test reaches. Coverage says the position was
instrumented and that no target executed it, so the run resolves the mutation
without starting a single process and reports it as surviving: nothing reached
it is a gap in the tests exactly as nothing noticed it is.

| Function | Reached | Fate |
| --- | --- | --- |
| `double` | yes | `mul-to-div` and `return-default` are killed by `doubling_two_is_four` |
| `half` | no | every mutation of it is `unreached`, and each raises a `surviving-mutant` finding a reviewer can accept |

This fixture exists because nothing else in the suite has an unreached
mutation, so the `unreached` column, the finding it raises, and the acceptance
that answers for it were carried by tests that returned early.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked --coverage`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --coverage
src/lib.rs:8:5 return-default killed
src/lib.rs:8:7 mul-to-div killed
src/lib.rs:8:9 int-decrement killed
src/lib.rs:8:9 int-increment killed
src/lib.rs:13:5 return-default unreached
src/lib.rs:13:7 div-to-mul unreached
src/lib.rs:13:9 int-decrement unreached
src/lib.rs:13:9 int-increment unreached
```
