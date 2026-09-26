<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-declines

One library crate whose tests decline to measure where the machine cannot hold what they ask of it, as [ADR 0043](../../docs/adr/0043-a-test-may-decline-to-measure.md) lets them.
A test declines by appending `<its libtest name>\t<why>\n` in one write to the file `RUST_MUTANTS_DECLINE_NOTICE` names, when a run named one, and returning; under a plain `cargo test` it only prints why.

| Item | Reached by | What a run establishes |
| --- | --- | --- |
| `can_share` | the two sharing tests, which then decline on every machine | nothing, where the mutation leaves it `false`: both tests decline again as the baseline's did; a mutation that makes it `true` has them measure, and they pass, which is a survivor |
| `can_measure` | `adds_ten`, which declines only when it answers `false` | a mutation that makes it answer `false` makes the test decline where the baseline did not, which is a detection; the others leave it `true` and survive |
| `shared` | `doubles_what_it_shares`, which then declines | nothing, except a mutation that panics before the test declines, which is a kill |
| `counted` | `counts_what_it_shares`, which then declines, and `counts_without_looking`, which never looks at the answer | survived, on the test that measured, with the decline recorded beside it |
| `measured` | `adds_ten` | killed |

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 int-decrement not_run
src/lib.rs:8:5 int-increment not_run
src/lib.rs:8:5 return-true survived
src/lib.rs:8:7 gt-to-ge not_run
src/lib.rs:8:9 int-decrement not_run
src/lib.rs:8:9 int-increment not_run
src/lib.rs:13:5 int-decrement killed
src/lib.rs:13:5 int-increment survived
src/lib.rs:13:5 return-true not_run
src/lib.rs:13:7 gt-to-ge survived
src/lib.rs:13:9 int-decrement survived
src/lib.rs:13:9 int-increment killed
src/lib.rs:18:5 return-default not_run
src/lib.rs:18:11 mul-to-div killed
src/lib.rs:18:13 int-decrement not_run
src/lib.rs:18:13 int-increment not_run
src/lib.rs:18:15 sub-to-add not_run
src/lib.rs:23:5 return-default survived
src/lib.rs:23:11 add-to-sub survived
src/lib.rs:23:13 int-decrement survived
src/lib.rs:23:13 int-increment survived
src/lib.rs:28:5 return-default killed
src/lib.rs:28:11 add-to-sub killed
src/lib.rs:28:13 int-decrement killed
src/lib.rs:28:13 int-increment killed
```
