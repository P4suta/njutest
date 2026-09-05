<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-rejectable

Candidates the compiler refuses, beside candidates it accepts. Rejection is
per mutant, with the compiler's own words attached, and it never costs a
sibling that compiles.

| Site | Rule | Fate |
| --- | --- | --- |
| `name + "!"` | `add-to-sub` | rejected, E0369 (`String - &str`) |
| `0..n` in `window` | `range-to-inclusive` | rejected, E0308 (`RangeInclusive` is not `Range`) |
| `Label("here")` | `return-default` | rejected, E0277 (`Label: Default` unsatisfied) |
| `value * 0` | `mul-to-div` | **accepted**: this compiler refuses only a division it can evaluate, and `value` is a run-time value. The mutant dies at run time instead |
| `greet`, `erase` tails | `return-default` | accepted |
| `a > b`, `a - b`, `b - a` | comparison, arithmetic, negation | accepted |

The `mul-to-div` row is the point of the fixture as much as the refusals
are: whether an edit is a program is a fact about the toolchain, read from
the toolchain, and never assumed from another language's rules.
