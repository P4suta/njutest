<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-coverage

Conditions a branch proof can be stated about, and conditions it cannot. The
proof is the licence to discharge a test that never took the branch; the point
of this fixture is that the licence is granted exactly where it is earned.

| Condition | Claim | Proof | Why |
| --- | --- | --- | --- |
| `value <= limit` | yes | yes | both operands are primitives, and the compiler vouches for it |
| `a <= b` on `Version` | yes | no | the comparison is a call into the program, and the witness is refused |
| `name <= "m"` on `&str` | yes | yes | the comparison is the library's: it runs none of the program's code, cannot panic, and terminates |
| `label == "target" && rank <= 3` | yes | yes | the two operands of a comparison are asked about separately, so a `String` beside a `&str` does not refuse the condition the `<=` claim rests on |
| `items.len() <= 2` | no | no | the condition runs the program's code, so the syntax claims nothing |

`tests/upper.rs` runs the condition of `clamp` and never the branch it gates,
so the branch proof the compiler vouches for and the coverage the run measures
together say that target cannot have noticed a mutation of the condition: it
is discharged rather than executed. The library's own tests run both branches
and are not.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked --coverage`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --coverage
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:14 le-to-lt not_run
src/lib.rs:9:16 return-default killed
src/lib.rs:11:5 return-default killed
src/lib.rs:16:8 negate-condition killed
src/lib.rs:16:10 le-to-lt survived
src/lib.rs:17:16 true-to-false killed
src/lib.rs:19:5 false-to-true killed
src/lib.rs:28:8 negate-condition killed
src/lib.rs:28:20 le-to-lt killed
src/lib.rs:28:23 int-decrement killed
src/lib.rs:28:23 int-increment killed
src/lib.rs:29:16 true-to-false killed
src/lib.rs:31:5 false-to-true killed
src/lib.rs:36:8 negate-condition killed
src/lib.rs:36:13 le-to-lt not_run
src/lib.rs:36:16 string-to-empty killed
src/lib.rs:37:16 true-to-false killed
src/lib.rs:39:5 false-to-true killed
src/lib.rs:44:8 negate-condition killed
src/lib.rs:44:14 eq-to-neq killed
src/lib.rs:44:17 string-to-empty killed
src/lib.rs:44:26 and-to-or killed
src/lib.rs:44:34 le-to-lt not_run
src/lib.rs:44:37 int-decrement survived
src/lib.rs:44:37 int-increment survived
src/lib.rs:45:16 true-to-false killed
src/lib.rs:47:5 false-to-true killed
```
