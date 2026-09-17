<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0020 — What a person is told is a value

## Status

Accepted, 2026-09-17 (user decision). Bounds every surface a person reads.

## Context

A run of `njutest verify` on a workspace with three gaps in it prints thirty
lines, and every one of them has the same weight. The three findings — the
only part anybody acts on — sit between ten `MUTANT` rows and three
`LIMITATION` rows, under five audit records that name the toolchain, the
repository, the scope and the timing. The gap itself is described and never
shown: a reader is told `src/lib.rs:8:10 gt-to-ge` and goes to open the file.

That is not an oversight. The terminal output *is* the record stream
([report-v1](../report-v1.md)): one record per line, tab-separated, the kind
first and the verdict last, so `tail -1` is the answer and a filter on the
first field is a projection. It is an excellent pipe format, and it is
excellent for the same reason it reads badly — it has no hierarchy, because a
filter does not want one.

The compiler solved this for the same data twenty years ago. `rustc` does not
print `src/lib.rs:8:10 E0308`; it prints the line, a caret under the span, the
type it wanted beside the type it got, and a `help:` that names what to do.
`clippy` adds the rule's name and `--explain`. Nothing about a surviving
mutant is harder to draw than a type error: the run already stored the path,
the line, the column, the byte span, the source digest, the original text and
the text it was replaced with.

The obstacle was never the drawing. It was that the output is produced by
three modules that each format a `Report` in their own way — `report/lines.rs`
writes the stream, `ui.rs` writes progress, `report/mod.rs` writes the
explanation — so a fourth way of drawing it would be a fourth place to keep
true, and the repository's own rule is that a second account of something is
one nothing keeps true.

There is also a constraint that reads as an obstacle and is not. The seam
policy ([ADR 0001](0001-seam-policy.md)) says only `main.rs` reads the process
environment, and how wide the terminal is, whether it takes colour, and
whether it is a terminal at all are things the environment knows. A renderer
that asked would be a seam.

## Decision

**What a person is told is a value, and every surface is a projection of it.**

One type — `presentation::Told` — holds what a run has to say: the verdict and
the numbers that support it, the diagnostics, and the sections a reader may
fold away. A diagnostic carries a severity, a code, a title, the span it is
about, the source excerpt at that span, the labels on it, the notes under it,
and the actions that answer it, each of which is a command somebody can run.

Every surface is a function of that value and nothing else:

| Surface | Signature |
| --- | --- |
| the record stream | `fn records(&Told) -> String` |
| the terminal | `fn human(&Told, Terminal) -> String` |
| the document | `fn json(&Told) -> String` |
| the briefing | `fn brief(&Told) -> String` |
| the review loop | `fn review(&Told, impl Answers) -> Reviewed` |
| the watch line | `fn moved(&Told, &Told) -> String` |

`Terminal` is what the composition root learned and passed in: the width,
whether colour is wanted, whether the font has the glyphs, whether stdout is a
terminal at all. The renderer never asks; `main.rs` answers, which is where
ADR 0001 has always put it.

The shape is guessed from where the output is going only when nobody said:
`--format` names it, and a run takes that at its word. The guess is right for
the two readers it was written for, a person at a terminal and a program
reading a stream, and wrong for the third, which is neither. Something that
will act on a run without a screen starts the same command through the same
pipe as a CI job does and is handed a record stream because of how it was
started rather than because of what it is. It is the one reader that can act
on what a run found and the one that could not ask for the shape that says
what to do about it.

## Consequences

**Every surface is testable without a terminal.** `human` is a pure function
of two values, so the same diagnostic is asserted at eighty columns and at
forty, with colour and without, in Unicode and in ASCII, in an ordinary unit
test with no process and no pty. That is the whole reason to do it this way,
and it is why the review loop takes its answers as an argument rather than
reading a keyboard — the pattern `njutest watch` already uses, where the look
and the round are the test's to supply so every rule of the loop is asserted
rather than waited on.

**One gallery shows the whole surface.** `cargo xtask gallery` renders every
diagnostic kind, every verdict, and every terminal shape into one golden file.
A change to the output is a diff a person reads, which is the only review of a
rendering that means anything.

**The record stream does not move.** It is a contract with programs, it is
what CI reads, and it keeps the guarantee report-v1 states. What changes is
that it stops being the thing a person is shown by default.

**A stale excerpt is refused rather than drawn.** The excerpt is read from the
file now and the run recorded the digest of the file then. When they differ,
the diagnostic says the file has changed rather than drawing a line that was
never the one the run measured — the same reason a locator names a mutation by
where it is rather than by a digest of the bytes around it.

**The cost is a layer.** Three modules that format a report become one that
builds a value and several that draw it, and until the last of them moves, the
old path and the new one both exist. The lint gate that refuses a second
account of the layout is the model for how that ends: the old formatter goes
when nothing calls it, and a test says nothing does.
