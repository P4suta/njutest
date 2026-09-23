<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-item-reach

A test that enters a function and leaves it before reaching any of its mutation sites.
`a_word_is_refused` calls `halve` with text that spells no number, and the `unwrap` panics before `n / 2` runs.
The guards at the sites of `halve` never see that test, and the entry marker at the first statement of `halve` does: entering an item and reaching a site in it are two different facts, and `njutest select` needs the first.

| Function | Shape | Entered by |
| --- | --- | --- |
| `halve` | the first site sits after a statement that can leave | `a_word_is_refused`, which reaches no site of it, and `half_of_four_is_two`, which does |
| `stop` | a body whose type is `!` | `stopping_stops` |
| `nothing` | a body with no statement | `nothing_does_nothing` |
| `next` | a body that is a single expression | `the_next_of_one_is_two` |
| `next_unchecked` | an `unsafe fn` | `the_next_of_one_is_two` |
| `later` | an `async fn`, entered when it is first polled | `later_is_twice_once_polled` |
| `thrice` | called only on a named worker thread still parked when the process exits, whose thread-local never drops | loose: no test thread entered it, and the record says a thread did |

The crate root says `#![deny(warnings, unused)]`, so an entry marker that drew a warning in any of these shapes would stop the instrumented build rather than pass unnoticed.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:7 div-to-mul killed
src/lib.rs:11:9 int-decrement killed
src/lib.rs:11:9 int-increment killed
src/lib.rs:24:5 return-default killed
src/lib.rs:24:7 add-to-sub killed
src/lib.rs:24:9 int-decrement killed
src/lib.rs:24:9 int-increment killed
src/lib.rs:32:5 return-default killed
src/lib.rs:32:7 add-to-sub killed
src/lib.rs:32:9 int-decrement killed
src/lib.rs:32:9 int-increment killed
src/lib.rs:37:5 return-default killed
src/lib.rs:37:7 mul-to-div killed
src/lib.rs:37:9 int-decrement killed
src/lib.rs:37:9 int-increment killed
src/lib.rs:42:5 return-default killed
src/lib.rs:42:7 mul-to-div killed
src/lib.rs:42:9 int-decrement killed
src/lib.rs:42:9 int-increment killed
```
