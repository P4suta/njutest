<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-subprocess

A library nothing calls in the test process. The one test runs the package's
own binary, which calls the library, so every region of `decide` is executed
by a process the test started rather than by the test itself.

| Function | Reached | Fate |
| --- | --- | --- |
| `decide` | through the binary | `negate-condition` and both `return` replacements are killed by `the_binary_names_both_sides_of_zero` |

This fixture exists because coverage is read out of the profiles a target's
processes wrote, and a profile is only readable against the binary that wrote
it. A run that reads a target's profiles against that target's own executable
alone loses everything its children did, and then reports code the tests do
execute as code nothing reaches.
