<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-shared-path

Two members that compile the same file through `#[path]`, each into a crate of
its own. `left` tests `within` and `right` tests `at_least`, so neither
member's suite can answer for the whole file.

| Path | Unit | Candidates |
| --- | --- | --- |
| `shared/util.rs` | the lib of both members | `within`: `return-true`, `le-to-lt`; `at_least`: `return-default`, `negate-condition`, `lt-to-le` |
| `left/src/lib.rs` | lib and test | never mutated: the module is a `#[path]` and a test module |
| `right/src/lib.rs` | lib and test | never mutated |

The file is one set of mutants and not two: a mutant is a place in a file, and
a file two crates compile is still one file. What differs is who can notice
one, so a run has to reach the targets of both members — the fates below say
so, because each function's mutants are killed only by the member that tests
it.

`lt-to-le` on `at_least` survives and always will: `if n < floor` and
`if n <= floor` return the same value for every input, because the only case
they decide differently is `n == floor`, where both answers are the same
number. It is an equivalent mutant and the ledger says so rather than hiding
it.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
shared/util.rs:8:5 return-true killed
shared/util.rs:8:7 le-to-lt killed
shared/util.rs:13:5 return-default killed
shared/util.rs:13:8 condition-to-false killed
shared/util.rs:13:8 condition-to-true killed
shared/util.rs:13:8 negate-condition killed
shared/util.rs:13:10 lt-to-le not_run
shared/util.rs:13:20 return-default killed
shared/util.rs:13:35 return-default killed
```
