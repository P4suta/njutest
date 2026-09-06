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
| `retries` | yes | the returned name already holds what the replacement would write, so every test that reaches it is discharged `never-infected` and the mutation is resolved without one execution |
| `tagged` | no | its equality answers about `number` while a test reads `tag`, so `Observable` is not stated for it and the compiler refuses the probe |

The invariant every run of this fixture must satisfy: for every mutant and
every test, if the test killed the mutant then the probe recorded that test
infecting it. A kill without an infection would mean a discharge could remove
a test that finds a defect.

`tagged` is here because that invariant once failed. Equality is what a probe
reads as "could this test have seen the replacement", so a `PartialEq` that
answers about less than a test can see makes the probe say nothing happened
while a test watches the difference. The probe is stated only for the types
whose equality is the whole of what a program can tell apart, and this fixture
is where a type outside that set is held to being refused.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 int-decrement killed
src/lib.rs:8:5 int-increment killed
src/lib.rs:8:5 return-default killed
src/lib.rs:13:5 return-default killed
src/lib.rs:13:7 mul-to-div killed
src/lib.rs:13:9 int-decrement killed
src/lib.rs:13:9 int-increment killed
src/lib.rs:18:5 return-default survived
src/lib.rs:23:5 return-default killed
src/lib.rs:37:9 return-default unreached
src/lib.rs:38:21 int-increment unreached
src/lib.rs:46:9 return-true unreached
src/lib.rs:46:21 eq-to-neq unreached
src/lib.rs:52:5 return-default killed
src/lib.rs:53:17 int-increment survived
src/lib.rs:54:14 string-to-empty killed
src/lib.rs:63:5 return-default survived
```
