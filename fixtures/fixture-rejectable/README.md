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
| `value * 0` | `mul-to-div` | rejected, `unconditional_panic` (deny by default) |
| `greet`, `erase` tails | `return-default` | accepted |
| `a > b`, `a - b`, `b - a` | comparison, arithmetic, negation | accepted |

The `mul-to-div` row is the point of the fixture as much as the others are.
`cargo check` accepts `value / 0` and `cargo test --no-run` refuses it: the
lint lives in the middle end and only fires once code is generated. Whether
an edit is a program is a fact about the toolchain, read from the toolchain
the way the run will use it, and never assumed.
