<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0013 — Codegen identity is the equivalence proof, and the premise is the runner's

## Status

Accepted, 2026-09-06. Implemented by `rust_mutants::equivalence` and
`mjutest_cli::assure::equivalence` (M10).

## Context

A mutation that compiles and means exactly what the code it replaced meant
cannot be killed by any test, because there is nothing to notice. Every
mutation testing tool reports such a mutation as surviving, and every one of
those reports is a finding that is not a gap in the tests. It is the oldest
unsolved problem in the field, and no tool solves it by proof.

Deciding semantic equivalence is undecidable in general. What is decidable is
a sufficient condition, and there is one sitting in the build directory.

## Decision

**The lemma.** Build the tree, build it again with one mutation spliced in, in
the same directory with the same flags, and compare the executables the two
builds produced. If every one of them is byte for byte the same file, the two
builds produced the same programs. The same program, run with the same
arguments in the same environment, makes the same observations. So no test can
tell the two apart.

The argument asks nothing of the compiler. It does not need it to be
deterministic, and it does not need it to be correct. It needs "the same bytes
behave the same way", and a determinism control — rebuilding the original after
every identity and checking it still builds to the bytes it built to — which
withdraws the layer's answers the moment that stops being something this
machine does.

**Linked artifacts, not intermediate representations.** A generic that this
crate never monomorphises, an `#[inline]` that only expands downstream, and an
LTO pass that crosses module boundaries each put a hole in a comparison of MIR
or of LLVM IR. There is no hole at the linked artifact: whatever the compiler
did, it finished doing it. `.rlib` and `.rmeta` are not compared, because they
carry MIR and would differ over a change that codegen erases.

**The profile is a parameter of the question.** The comparison is made under
the project's own test profile. `x + 0` and `x - 0` are the same instructions
at `opt-level = 1` and different ones at the `opt-level = 0` cargo gives a test
profile by default, and the profile the tests run under is the one that
decides. Normalising at `-O3` would raise the yield and would be answering a
different question than the one the tests ask.

**The engine says `identical`; the runner says `equivalent`.** This is the
decision the whole layer turns on. `--gc-sections` is on by default, so a
mutation of a function no test calls is dropped by the linker and the
artifacts come out identical — for the exact opposite of a reassuring reason.
Reading that as equivalence would delete the finding that says the code is
untested, which is the finding mutation testing exists to raise.

So the engine states one fact and never a verdict, and the runner holds it to
premises the engine has no way to check:

1. a control has not withdrawn the layer;
2. no test wrote into the tree while it was being measured;
3. the package holds no `unsafe`, where "the same instructions" and "the same
   behaviour" stop being one sentence;
4. the route was decided by region and named at least one target — **the tests
   ran the position**;
5. this run recorded no killer, which every survival satisfies by construction.

This is [ADR 0004](0004-proof-layers-not-budgets.md) decision 3 in the shape
this layer takes: the lemma and the premise live on different sides of the
boundary, and the side holding the evidence is the side that decides.

**It is not a budget.** The layer removes no execution: by the time it runs,
every test that reaches the mutation has already run and the mutation has
already survived. What it removes is a finding, and only where nothing could
have found it. Turning the layer off leaves the finding in place, which is the
fail-closed direction.

**One switch, no partial runs.** `[mutation] equivalence` is a boolean, and
when it is on every survivor whose premises hold is asked about. A layer that
stopped at a budget would make two runs of the same tree report different
findings, which breaks `xtask report-diff` and the rule that a run says in its
recording why it was faster.

## Consequences

- `equivalent` is a disposition and a column of its own, not a part of
  `survived`: a reader who cannot tell "nobody noticed this" from "nobody
  could have" cannot act on either. The accounting identities gain it.
- The cost is two builds of a tree of its own per survivor whose premises
  hold — the mutated build, and the control. They are incremental, so what
  each costs is the mutated crate and what depends on it.
- On a project that leaves `[profile.test] opt-level` at cargo's default of
  zero the yield is close to nothing, and `docs/limitations.md` says so.
- The engine's tree for this layer is the program the user wrote: no guards,
  no runtime module, no instrumentation. Proving something about an
  instrumented tree would be proving it about a different program.
