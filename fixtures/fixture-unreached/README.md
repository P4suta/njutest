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
