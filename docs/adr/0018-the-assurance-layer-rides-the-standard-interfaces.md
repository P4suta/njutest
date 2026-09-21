<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0018 — The assurance layer rides the standard interfaces

## Status

Accepted, 2026-09-09 (user decision). Bounds every milestone after M14.

## Context

`njutest` and `rust-mutants` are one workspace with a fixed dependency
direction ([ADR 0012](0012-one-workspace-two-products.md)), and the runner has
grown a layer that finds test binaries, names them, and runs them. That layer
looks like a second implementation of `cargo nextest`, and the question was
whether to invert the arrangement: make `njutest` a general test runner, with
mutation testing as one plugin among the ways people test.

The reason to want that is sound. Mutation testing says nothing without tests,
so something has to own the general facts about a suite, and that owner would
be a natural home for property-based testing, fault injection, and whatever
else comes. The reason not to is the same sentence read the other way: what
those methods contribute is how a test is *written*, and a layer that owns how
tests are written owns a syntax. This repository is not going to invent one.

Measuring the supposed duplication changed the picture. `assure/schedule.rs`
is 113 lines and none of them schedule tests: it decides how many mutations
are measured at once and puts the answers back in catalog order.
`targets.rs` is mostly the target identity `docs/report-v1.md` promises, which
another runner cannot supply because it has an identity of its own. What is
left is `build.rs` reading cargo's JSON artifact messages — which is what
every runner does, `nextest` included, because it is the only interface cargo
offers.

One constraint decides the rest. `nextest` runs each test in its own process;
that is its central design choice and where its isolation comes from. The
inner loop of a mutation run needs the opposite: many tests in one process
under one activation, which is what makes the schemata form cheap at all.
Handing that loop to a per-test runner multiplies process starts by however
many tests a route names per pair. This workspace measures itself:

```
WORK  started=825 of 19074 pairs across 66 targets; 95.7% removed
      tests=4064 of 172244; 97.6% removed
```

825 processes carry 4064 tests, so the factor is 4.9 here — and it is 9.0
against the catalog before routing, because a proof that removes work removes
whole pairs while the tests inside the pairs that remain stay where they are.
The better the layers get, the worse the trade becomes. This is arithmetic,
not preference.

The engine is already on the standard interface for the same loop, and the
interface is the harness rather than a runner: routing at `test` granularity
hands one process several test names as position filters, which is libtest's
own command line doing what it documents.

## Decision

**`njutest` is an assurance layer, not a test layer.** It does not become a
test runner, does not host plugins, and does not define a way to write a test.
What it sells is the verdict and what stands behind it: the proof layers, the
independent re-derivation in `cargo xtask proofaudit`, evidence reuse, and the
contract each of those answers to.

**Mutation is one evidence source among several, and that shape already
exists**: `assure/mutation.rs`, `assure/deep.rs` (Miri), `assure/fuzz.rs`,
`assure/repair.rs`, and the verification driver K1 adds are siblings under
one `assure/`. Integrating a further method means adding a sibling, not a
plugin interface. An interface with one implementation is a promise nobody
asked for.

**The only interfaces to the test world are the two standard ones**: cargo's
JSON artifact messages, and libtest's own command line. The run starts the
test binaries itself through the second of those, which is what routing at
`test` granularity already does — a process handed several test names as
position filters is libtest's command line doing what it documents.

**What somebody has to install to use this is cargo, and nothing else**
(user decision, 2026-09-09). No third-party test runner is a dependency, and
none is an option either. A tool with a chance of becoming part of a
language's own infrastructure hands its dependencies to everybody who adopts
it, so the question about one is never "is it good" — `nextest` is very good —
but "should every project that wants an assurance verdict also have to want
this". The answer has to be no for anything that is not already in the
toolchain. The optional integrations that exist — Miri, `cargo-fuzz`, and the
verifier K1 adds — are each the subject of a contract that names them
(`deep-v1`, `verified-v1`), and a run that does not promise them runs without
them.

**A target's identity is this repository's own.** `docs/report-v1.md` defines it, and a report is compared against other reports of the same tree; borrowing another tool's names would make the identity theirs to change.

## Consequences

- A new testing method arrives as a phase under `assure/` with its own
  contract clause, trace vocabulary, and audit layer, exactly as
  [ADR 0004](0004-proof-layers-not-budgets.md) requires of a proof. Nothing
  about it is configured in a syntax of ours.
- `build.rs` and `targets.rs` stay. They are not a competitor to anything;
  they are the standard interface plus the identity a report promises.
- Nothing gets faster by adopting somebody else's runner, and that door is
  closed rather than left ajar. What is left to make the baseline cheaper is
  the baseline itself, which is one pass over every target and is the floor a
  mutation score cannot be had without.
- A new optional tool arrives the way Miri did: as a contract that names it,
  refuses to conclude without it, and is not what a default run promises.
- The question of a plugin API is closed. Reopening it means arguing that a
  second engine exists to plug in.
