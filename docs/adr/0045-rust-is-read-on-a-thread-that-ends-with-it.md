<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0045 — Rust is read on a thread that ends with it

## Status

Accepted, 2026-09-26.
Implemented by `rust_mutants::parsing::{apart, Parsing, ReadingError}`, every reading of Rust text in the engine and the runner, the `raw-lexing` lint, and the laws in `crates/rust-mutants/tests/parsing.rs` and `crates/njutest/tests/soundness.rs`.

## Context

The engine and the runner read Rust with `syn`, which lexes through `proc-macro2`.
Built with `span-locations`, as the engine is so that a span names the bytes a mutant is written over, `proc-macro2` keeps every text it lexes, and its line table, in a map local to the thread that lexed it.
Nothing frees an entry while the thread lives, and the map's locations are 32 bits: past 4 GiB of text on one thread, every location wraps.
In a debug build that is a panic; in a release build `Span::byte_range` answers with a place in some other text, which is where a mutant would then be written.

The engine read its sources on whatever thread called it.
A run reads each file several times — discovery, its tokens, the instrumented file read back, every alternative folded onto one line — and until the operator-swap check was held to its own item, every swap that changes binding strength read the whole file again, three times over: 0.40 GB for this repository's 10.9 MB.
`njutest watch` runs every round on one thread and reads the whole workspace in each, so a watch session kept every round's sources for as long as it ran, and wrapped after enough of them.

## Decision

**Rust text is read only through `rust_mutants::parsing`.** `apart(work)` starts a named thread with a 64 MiB reserved stack, lends `work` a `Parsing`, joins the thread, and hands back what `work` made.
`Parsing::{read, read_with, file, tokens}` are the only ways text becomes tokens.
Every entry point that reads — discovery, instrumentation and its parts, the crate-root checks, the skeleton, the change selection, the soundness inventory, the concurrency scan, the model harness — reads inside an `apart`, and passes the `Parsing` down to what it calls.

**The type system keeps a location from outliving its map.** `apart` returns only what is `Send`, and nothing that carries a `proc-macro2` location is: a `Span`, a `TokenStream`, a `LexError` and every syntax tree hold `proc-macro2`'s `!Send` marker.
`Parsing` is lent by reference and cannot be copied, and it is `!Send` too, so the right to read cannot leave the thread whose map it fills.
`syn::Error` is `Send` and keeps its location only on the thread that made it, so no reading returns one: `ReadingError` is converted on the reading thread, and carries a line and column rather than a span.

**A thread's reading is budgeted before a byte is lexed.** `Parsing` carries what its thread has spent and may spend; each read is charged three times its length plus one, covering the literals `syn` lexes a second time and the gap left between texts, and past half of the 32-bit space it is refused as `ReadingError::Exhausted` (RM0018), never wrapped.
A reading that fails for that reason, or because its thread could not start (RM0019), is not an answer about the text: discovery fails the file by name, instrumentation says why, and a check that already answers conservatively for a file that does not parse — the skeleton seals nothing, the scan assumes an include, the selection shadows every name — takes that same answer.

**A lint holds every reading there.** `raw-lexing` refuses, in shipped code outside `parsing.rs`, `parse_str` and `parse_file` however they are named, called or passed, a `TokenStream` or `Literal` `from_str` and an inferred `FromStr::from_str`, a `.parse::<T>()` of a type not on a list of types that are not Rust text, `parse_with` and a parser's `parse_str`, `LitInt::new` and `LitFloat::new`, and `quote!` and its kin, which lex every literal they quote.
The macros crate, which is handed the compiler's own tokens, the devkit, which only measuring code links, and code compiled only for tests are outside it.

## Consequences

- A reading's locations, and the copy of its text, end with the thread that read it: ten discoveries leave the caller's map one probe larger, and so does every other entry point, in the engine and in the runner.
- A watch session holds no round's sources after the round.
- A file that would take its reading past the budget is refused by name, where it used to be read to wrong places or, before that, to panic.
- A long chain of operators reads on the reading thread's stack, which is larger than a main thread's; a 6000-term chain that aborts on 8 MiB reads.
- A panic in a reading is raised again in the thread that asked for it, so under the test profile it unwinds there as any panic would, and in a release build, which aborts on a panic, it ends the process; no value ever stands in for one.
- Each reading starts a thread: a few hundred per run, against the build of the tree it reads.
- A reading inside a reading is a thread and a budget of its own, since the budget travels with the right to read rather than living in the thread; nothing does that on a hot path.
- A new kind of text a `.parse::<T>()` reads is a line on `PARSED_TYPES`, which is where someone decides it is not Rust.
- `xtask` reads the repository with `syn` on its own threads, outside this rule: it cannot depend on the engine, and one run reads the tree once.
