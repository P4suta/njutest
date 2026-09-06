<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Proof layers

**Status: implemented.** A run removes an execution only where something
proves the execution could establish nothing. Every removal is named, every
removal is recorded, and every removal is re-derivable from files the run
keeps. Nothing here is a budget, a sample, or a guess; see
[ADR 0004](../adr/0004-proof-layers-not-budgets.md).

## The layers

| Layer | Lemma | Premise | Removes |
| --- | --- | --- | --- |
| coverage routing | — | this target's measured run covered no region holding the mutation | the (mutant, target) pair |
| `branch-never-taken` | the compiler: this mutation changes nothing outside the body the condition gates | this target's measured run covered no region beginning inside that body | the (mutant, target) pair |
| `never-infected` | the probe: this test ran the mutation and its value never differed | the probe log this target's own run appended to | the (mutant, target) pair |

A mutation every target is removed from is not executed at all: `unreached`
when the measurement placed it and nothing ran it, `discharged` when a proof
took every target away. Both are findings — the tests have a gap where the
mutant is — and neither is a survivor, because nothing measured it.

## A proof without a premise removes nothing

The lemma is the compiler's or the probe's; the premise is always the
measurement's. `--no-coverage` measures nothing, so it discharges nothing,
whatever the compiler vouched for. A target whose profile could not be read is
one the measurement says nothing about, so it is routed to and never
discharged: a proof resting on its silence would rest on the measurement's
failure.

`prove::discharges(proof, path, covered)` is the pure function of the two. A
caller with its own coverage — `mjutest` is one — discharges with its own
evidence by calling it, and an audit re-implements it rather than asking the
engine.

## What a run keeps

A report that names a discharge without the premises is a claim rather than a
proof. Beside its report a run writes:

| File | What it holds |
| --- | --- |
| `reached-v1.json` | every region each target's measured run covered, every region the build instrumented, and what the measurement could not establish |
| `catalog-v1.json` | every mutant, with the body of the branch the compiler vouched for |
| `probe/<target>.log` | what each probe process appended |

An `evidence` recording names each with its size and digest.
`cargo xtask engine-audit <run>` reads them and re-decides every discharge
without the engine: a target that covered a region inside the body it was
discharged from is a violation, a discharge whose premises the run did not
keep is unaudited, and a discharged pair the recording then executed is a
violation.

## After the run

`--equivalence` asks the compiler whether each survivor's mutation is one it
renders at all: the tree the user wrote is built once, the mutation is spliced
in, and the two builds' executables are compared byte for byte. An answer of
`identical` says the compiler produced the same program, and the control is
built again to check that the tree builds reproducibly at all — a tree whose
build is not reproducible proves nothing, and one such answer withdraws every
answer afterwards.

It never says `equivalent`. Two binaries being the same bytes is a fact about
what the compiler produced under the profile the tests run; whether the
mutation could change behaviour is a question about the program, and a
comparison of binaries does not answer it
([ADR 0013](../adr/0013-codegen-identity-is-the-equivalence-proof.md)). A
mutation the compiler refuses establishes nothing either: the question is
about two programs, and there is only one.

## Where a layer is silent

`llvm-cov` regions nest. What says a body ran is a region that *begins* inside
it: the region of the function that holds the branch contains the body and
says the function ran, and the region at the body's closing brace is the one
the compiler emits for what follows the branch. Reading containment as
execution would discharge nothing; reading overlap as execution would
discharge everything a function's own region touches. Neither is what the
measurement says.

A library's documented examples are compiled by rustdoc while cargo runs them,
so no coverage build instruments them: a mutation is routed to a documentation
target by the file it is in (`doctests-routed-by-file`), which is wider than a
region and is the direction a fallback must go.
