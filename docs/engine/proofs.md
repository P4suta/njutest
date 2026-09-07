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

## The unit of work

A run's cost is counted in **pairs**: one mutant asked of one target, which is
one test process started. A run that asked every target about every mutant
would start `mutants × targets` of them, and every pair short of that is one
something removed.

The unit is a count and not a duration on purpose. A second is about this
machine, this load, this job count; it cannot be compared between two runs and
it cannot be ratcheted. A pair is the same number everywhere, so a change that
makes the engine do less is a change a test can see —
`xtask/work_ceiling.txt` holds each fixture's count, and like the seam
allowlist it may shrink and never grow.

```
WORK  started=11 of 33 pairs across 3 targets; 66.7% removed
      (unreached=20 answered=2)
```

`rust_mutants::work::Work` derives that from the stored report alone, so an
audit re-derives it without the engine, and `engine-audit`'s `work` layer
holds the total to the `mutant-exec` records the recording kept. Every removal
is labelled with what kind it is:

| Kind | What it means | Still the whole answer |
| --- | --- | --- |
| proof | something proved the pair could establish nothing | yes |
| sufficiency | a target had answered already | yes |
| memory | an earlier run of the same tree established it | yes |
| selection | the run was asked for less than the whole | **no** |

`Work::answers_for_the_whole` is that last column, and a report says so when
it is false.

## Whether the removals are honest

Every layer here claims that running the pair would have established exactly
what the run reports without it. That is a falsifiable claim, and
`crates/rust-mutants-cli/tests/toolchain_differential.rs` falsifies it: four
fixtures, run twice — once with the measurement and the proofs on, once with
`--no-coverage` so nothing is removed — and the two reports held to each other
mutant by mutant. A mutant the proved run never started a process for has to
be one the whole run found nothing noticed.

The test refuses to be vacuous: it fails if no fixture cost less with the
layers on, and it fails if no mutant was removed by a proof at all. It takes
about six seconds, which is why it runs with every other test rather than
weekly.

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
