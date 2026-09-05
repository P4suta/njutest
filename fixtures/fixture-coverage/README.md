<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-coverage

Conditions a branch proof can be stated about, and conditions it cannot. The
proof is the licence to discharge a test that never took the branch; the point
of this fixture is that the licence is granted exactly where it is earned.

| Condition | Claim | Proof | Why |
| --- | --- | --- | --- |
| `value <= limit` | yes | yes | both operands are primitives, and the compiler vouches for it |
| `a <= b` on `Version` | yes | no | the comparison is a call, and the witness is refused |
| `items.len() <= 2` | no | no | the condition runs the program's code, so the syntax claims nothing |
