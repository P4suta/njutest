<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-reads-its-source

A library that holds the text of one of its own modules, and a test that reads the same module as text.

| File | What it reads | Fate |
| --- | --- | --- |
| `src/twice.rs` | nothing; it is the module the others read | every mutation of `value * 2` is killed by `three_doubled_is_six` |
| `src/lib.rs` | `src/twice.rs`, with `include_str!("twice.rs")` and with `include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/twice.rs"))` | no candidate |
| `tests/source.rs` | `src/twice.rs`, with `include_str!("../src/twice.rs")`, and compares every reading with the file as it is written | no candidate |

This fixture exists because a run used to refuse it.
The engine rewrites `src/twice.rs` to carry its guards, and every include of it read the rewritten file, so `tests/source.rs` failed with nothing active and the run stopped with `RM5002`.
Now each include of a Rust source of the tree reads a copy of that source as it was copied, beside the file that reads it, and the trace says so with a `verbatim` note for each.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/twice.rs:8:5 return-default killed
src/twice.rs:8:11 mul-to-div killed
src/twice.rs:8:13 int-decrement killed
src/twice.rs:8:13 int-increment killed
```
