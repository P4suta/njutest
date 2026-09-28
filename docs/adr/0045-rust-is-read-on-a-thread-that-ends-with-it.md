<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0045 — Rust is read on a thread that ends with it

## Status

Accepted, 2026-09-26.
Implemented by `rust_mutants::parsing::{apart, Parsing, ReadingError, Depth}`, every reading of Rust text in the engine and the runner, the `raw-lexing` lint, and the laws in `crates/rust-mutants/tests/parsing.rs`, `crates/rust-mutants/tests/depth.rs` and `crates/njutest/tests/soundness.rs`.

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

**A thread's reading is budgeted before a byte is lexed.** `Parsing` carries what its thread has spent and may spend, and past half of the 32-bit space a reading is refused as `ReadingError::Exhausted` (RM0018), never wrapped.
Each is charged exactly what `proc-macro2` takes of the space, a position for every byte and one left after the text, before it is lexed.
The one text a reading does not lex itself is a negative number `syn` reads back to split its sign from its digits, as it peeks at it and takes it: once a text is lexed and before a tree is built of it, every number a `-` stands right before is charged `READ_AGAIN` (3) times at its length, its sign and the position after it.
A law holds `syn` to that count in every context that reads such a number, and a generic argument, which reads one back three times, holds the count to no more than that.
An operator swap holds itself to the tree it names by reading back its own edit: the unit it stands in is lexed once, each edit alone, spliced into the unit's tokens where the lexer would join them, and a swap inside a statement that ends with a `;` or ends its block is parsed back as that statement alone, which reads as its block reads it.
So what holding a file's swaps reads back grows with the file rather than with the square of its longest item: a 95 KB function of 4,500 lines was refused after 137 s when each swap read its whole item back at three times its length, and a 126 KB function of 4,500 statements holding 9,000 such swaps now reads back 0.8 MB and discovers in half a second in a debug build.
**A reading measures how deep its text runs before anything recurses through it.** The lexer builds its tokens without recursing, and every consumer after it does recurse: the parser, `syn`'s token buffer, the walks, the clone and the drop go one frame deeper for every group, and for every link of a chain the parser nests one tree inside the last.
So `Parsing` walks the lexed tokens once with a stack of its own and refuses, as `ReadingError::TooDeep` (RM0020), a text whose groups nest past `NESTING` (1,000), and, where the reading builds a tree, one where a path through its trees passes more than `CHAIN` (12,288) tokens.
The chain a path passes is, at each group it enters, every token of the run that group sits in; a run ends at `;`, `,` and `=>`, and at a closing brace that no `as`, `else`, punctuation or group goes on from, which is where a list the parser keeps flat — items, statements, arms — moves on to its next element, and an attribute counts apart from the run it sits in, since the parser keeps attributes in a list of their own.
A brace that something goes on from does not end the run, so `x + if {c} {1} else {2} + …` is one chain however many braces it passes, and each of its links is counted.
The bounds were measured, not guessed: in a debug build, where frames are largest, a reading thread's 64 MiB overflowed at 12,300 nested blocks, the costliest group, and at 25,000 chained `return`s and 8,400 nested generic arguments, the costliest links, about 5.3 KiB a group and 2.7 KiB a token; both bounds at once take about 38 MiB.
Laws read a text at each bound and at both at once, through discovery, item numbering and the skeleton, and refuse one a group or a token past each; a text a reading only lexes is refused only for its nesting, since no tree is built from it.

A reading that fails for any of those reasons, or because its thread could not start (RM0019), is not an answer about the text: discovery fails the file by name, instrumentation says why, and a check that already answers conservatively for a file that does not parse — the scan assumes an include, the selection shadows every name — takes that same answer.
Every error that carries a reading's failure holds the `ReadingError` itself in a named `source`, made by its one `From<ReadingError>`, and answers with the reading's code: instrumentation turns each error it wraps into its own through one exhaustive match, so a refusal the reading made is never told as a source that changed under the run (RM3002) or a defect of the engine (RM3004), and the runner carries the code of a source its model phase could not read rather than its own.
A law feeds a text too deep to read to every way into the engine that reads Rust and holds each to RM0020.
The skeleton is the check whose answer for text that is not Rust is the permissive one: a data file declares nothing another file sees.
So it keeps three answers apart in a closed `Read { File, NotRust, Unread }` that each of its rules matches in full, with no default any call site supplies, and a file it could not read unseals every body of its unit as `unit-file-unread` rather than reading as a file that declares nothing.

**The compiler and a lint hold every reading there.** Clippy refuses, everywhere in the workspace, every `syn` function that lexes the text it is given — `parse_str`, `parse_file`, a parser's `parse_str`, `LitStr::parse` and `parse_with`, `LitInt::new` and `LitFloat::new` — and `quote!` and its kin, which lex every literal they quote, however each is named, imported, called, passed or written inside another macro's arguments, since it resolves what a path names rather than how it is spelled.
The doors past it are named, each with the one waiver it needs: `njutest_devkit::lexed` for code that measures, tests, benches and examples among it, xtask's `lexed` for the repository's gates, and the macros crate, which is handed the compiler's own tokens, for `quote!`.
`raw-lexing` holds what the compiler cannot tell apart from what it must allow — a `TokenStream` or `Literal` `from_str`, an inferred `FromStr::from_str`, a `.parse::<T>()` or `str::parse::<T>` of a type not on a list of types that are not Rust text, and a `str::parse`, or a `.parse()` inside a macro's arguments, that names no type at all — and names the `syn` functions and macros too, in every crate's sources, build script, benches and examples outside `parsing.rs`, the macros crate and the devkit.
It reads a name the file imports under another, or gives a type with a `type` alias, as the name it stands for, and the arguments of every macro as the tokens they are, so a lexing a renamed import, an alias or a `format!` hides is found, and each shape it finds is planted where the gate is checked.
Code compiled only for tests is outside the lint, and goes through the devkit's door for the compiler.

## Consequences

- A reading's locations, and the copy of its text, end with the thread that read it: ten discoveries leave the caller's map one probe larger, and so does every other entry point, in the engine and in the runner.
- A watch session holds no round's sources after the round.
- A file that would take its reading past the budget is refused by name, where it used to be read to wrong places or, before that, to panic; since the charge is what a reading takes, the same ceiling reads about three times the text a charge of three times its length did.
- A long chain of operators reads on the reading thread's stack, which is larger than a main thread's; a 6000-term chain that aborts on 8 MiB reads.
- A text nested or chained past the bounds is refused by name as RM0020, where it used to overflow the reading thread's stack and abort the process with nothing to say which file did it; the scan of the runner reads the same measure, so it refuses at the engine's nesting rather than the 128 it kept for itself.
- A panic in a reading is raised again in the thread that asked for it, so under the test profile it unwinds there as any panic would, and in a release build, which aborts on a panic, it ends the process; no value ever stands in for one.
- Each reading starts a thread: a few hundred per run, against the build of the tree it reads.
- A reading inside a reading is a thread and a budget of its own, since the budget travels with the right to read rather than living in the thread; nothing does that on a hot path.
- A new kind of text a `.parse::<T>()` reads is a line on `PARSED_TYPES`, which is where someone decides it is not Rust.
- `xtask` reads the repository with `syn` on its own threads, outside this rule: it cannot depend on the engine, and one run reads the tree once.
