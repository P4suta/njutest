<!--
SPDX-FileCopyrightText: 2026 njutest contributors
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
| `name_of`, `items_of`, `maybe_of` | yes | the standard library's own `String`, `Vec` and `Option`: coherence lets nobody give them another `PartialEq`, so equality is the whole of what a program can tell apart. Every test leaves each at the default, so each is discharged `never-infected`, and narrowing the trait back to the primitives makes all three survive |
| `no_name` | yes | `&str` has a `Default`, and the returned value is one |
| `borrowed_name` | no | the returned value is a `&String`, which has no `Default` however much it coerces to `&str` at the return. The trait covers a reference to anything it covers; `Default` is what the value has to have as well |

The invariant every run of this fixture must satisfy: for every mutant and
every test, if the test killed the mutant then the probe recorded that test
infecting it. A kill without an infection would mean a discharge could remove
a test that finds a defect.

`tagged` is here because that invariant once failed. Equality is what a probe
reads as "could this test have seen the replacement", so a `PartialEq` that
answers about less than a test can see makes the probe say nothing happened
while a test watches the difference. The probe is stated only for the types
whose equality is the whole of what a program can tell apart — the primitives,
`str`, `String`, and `Option` or `Vec` of one of those — and this fixture is
where a type outside that set is held to being refused.

No tree is built for any of this. The guard the instrumented tree already
holds compares the value the branch that keeps the original produced against
the constant the replacement writes, on the run that verifies the baseline
([ADR 0016](../../docs/adr/0016-the-probe-tree-is-a-tree-nobody-needs.md)).

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
src/lib.rs:63:5 return-default not_run
src/lib.rs:80:5 return-default not_run
src/lib.rs:86:5 return-default not_run
src/lib.rs:92:5 return-default not_run
src/lib.rs:92:5 return-some-default killed
src/lib.rs:98:5 return-default survived
src/lib.rs:107:5 return-default not_run
```
