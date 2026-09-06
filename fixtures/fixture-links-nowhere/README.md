<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-links-nowhere

A crate that type-checks and does not link: it calls a symbol no library
supplies, which only the linker finds out.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | never reached | the run is refused before anything is proposed |

A check answers "is this a program"; it does not answer "does this link". A
run whose gate was only a check accepted this tree, instrumented it, and then
failed every validation round — where the failure reads as a mutation the
compiler refused, and bisection goes looking for which one. It is neither, and
it is the same failure `cargo test` gives. The gate is a check of the whole
workspace and then a test build of the packages the run is about, so this tree
is refused with `RM5001` before any round.

`list` and `why-skipped` still only type-check: they rule on nothing, so they
are answerable for a tree that does not link, and asking a linker for an
answer nobody reads is work nothing needs.

## Fates

No run of this fixture ever proposes a mutant: it is refused at the gate, so
the ledger below is empty and stays empty. `cargo test -p rust-mutants-cli
--test toolchain_fates` still checks it, which is what says the refusal is
still a refusal.

```fates
```
