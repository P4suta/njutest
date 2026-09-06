<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest

A library with one documentation example. The run compiles and runs the
documentation as one target for the library, so a broken example is a failing
test rather than something nobody looked at, and states
`doctests-not-routed`: the target carries no coverage, so no mutation is
routed to it and no mutation is answered by it.

| Function | Documented | Fate |
| --- | --- | --- |
| `double` | with an example | `mul-to-div` and `return-default` are killed by `doubling_two_is_four`; the example is a target of its own and answers for nothing |
| `half` | without one | every mutation of it is `unreached`, which the limitation qualifies: the documentation is not among the tests that were routed |

This fixture exists because the contract says documentation examples are run
and classified as one target per library, and nothing measured whether they
were.
