<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-include

A library assembled out of three files, two of which no `mod` declares:
`include!` pastes one where items go and one where an expression goes.

| File | How it is pasted | Fate |
| --- | --- | --- |
| `src/lib.rs` | it is the library | `return-default` on `total` is killed; the `const` initializer is a `const-context` skip and the `include!` in it a `macro-invocation` skip |
| `src/items.rs` | at item position | `return-true` on `over` is killed; `gt-to-ge` survives, because `n >= 10` and `n > 10` differ only at ten and no test passes ten |
| `src/table.rs` | at expression position | `included-expression`: it is a fragment rather than a program, so nothing parses it and nothing appends a runtime module to it |

This fixture exists because both halves used to be wrong, and both were
wrong quietly. A file pasted in at expression position does not parse as a
set of items, and discovery stopped the whole run with a syntax error over a
project the compiler is perfectly happy with. A file pasted in at item
position brought its runtime module into the includer's scope, where the
includer's module of the same name already was, and every mutant of both
files came back `compile-rejected` — the tool's own breakage, reported as
the compiler's judgement about the user's code.

The library's own documentation is this file, pulled in with
`#[doc = include_str!]`. A dep-info names every file a compilation depended
on, so this markdown is in there beside the Rust — and reading it as Rust,
which is what discovery used to do with everything a dep-info named, failed
the whole run.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/items.rs:9:5 return-true killed
src/items.rs:9:7 gt-to-ge survived
src/lib.rs:14:33 sum-to-product killed
src/lib.rs:15:5 return-default killed
src/lib.rs:15:8 negate-condition killed
src/lib.rs:15:18 return-default killed
src/lib.rs:15:22 mul-to-div killed
src/lib.rs:15:35 return-default killed
```
