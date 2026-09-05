<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-probeable

Return replacements a probe can answer for, and ones it cannot. A test that
never infected a mutant cannot have killed it, and this fixture is where that
is checked against real executions rather than argued about.

| Function | Probed | Why |
| --- | --- | --- |
| `seven` | yes | the returned expression is a literal, so evaluating it again is not an event |
| `double` | yes | `n * 2` is not probed, but the returned expression of the mutation is; `doubling_zero_is_zero` returns the default and infects nothing, `doubling_four_is_eight` does not and infects |
| `measured` | no | `items.len()` is a call, and a call may do anything the second time |
| `ratio` | no | `-0.0` equals `0.0` and is not what the default writes, so the compiler refuses the probe |

The invariant every run of this fixture must satisfy: for every mutant and
every test, if the test killed the mutant then the probe recorded that test
infecting it. A kill without an infection would mean a discharge could remove
a test that finds a defect.
