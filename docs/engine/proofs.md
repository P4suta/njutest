<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Proof layers

**Status: implemented.** A run removes an execution only where something proves the execution could establish nothing.
Every removal is named, every removal is recorded, and every removal is re-derivable from files the run keeps.
Nothing here is a budget, a sample, or a guess; see [ADR 0004](../adr/0004-proof-layers-not-budgets.md).

## The unit of work

A run's cost is counted in **pairs**: one mutant asked of one target, which is one test process started.
A run that asked every target about every mutant would start `mutants × targets` of them, and every pair short of that is one something removed.

A pair is a process, and a process is not the whole of what a run does: one that runs the two tests that reached a mutation costs less than one that runs the target's two hundred.
So the ledger counts **tests** as well, and that is the number the guards move ([ADR 0014](../adr/0014-the-guards-are-the-measurement.md)).
Every test a run started is in it, including the ones it started to establish that a filtered set answers on its own.

The unit is a count and not a duration on purpose.
A second is about this machine, this load, this job count; it cannot be compared between two runs and it cannot be ratcheted.
A pair is the same number everywhere, so a change that makes the engine do less is a change a test can see —
`xtask/work_ceiling.txt` holds each fixture's count, and like the seam allowlist it may shrink and never grow.

```
WORK  started=15 of 42 pairs across 3 targets; 64.3% removed
      (unreached=25 answered=2)
      tests=18 of 70; 74.3% removed (3 of them establishing that a
      filtered set answers on its own)
```

`rust_mutants::work::Work` derives that from the stored report alone, so an audit re-derives it without the engine, and `engine-audit`'s `work` layer holds the total to the `mutant-exec` records the recording kept.
Every removal is labelled with what kind it is:

| Kind | What it means | Still the whole answer |
| --- | --- | --- |
| proof | something proved the pair could establish nothing | yes |
| sufficiency | a target had answered already | yes |
| memory | an earlier run of the same tree established it | yes |
| selection | the run was asked for less than the whole | **no** |

`Work::answers_for_the_whole` is that last column, and a report says so when it is false.

## What a measurement must name

A measurement removes a target from a route by saying that target ran and covered nothing there.
It can only say that about a target it **named**. A target absent from the measurement — its profile unreadable, its run never made, the measurement cut short before reaching it — is one nothing was established about, and it stays in every route.

Being absent from a measurement is not the same as being measured and covering nothing, and reading the first as the second turns a kill into a survivor.
That is the one thing this layer must never do, and it is the one thing it did: a run interrupted mid-measurement remembered what it had, and every later run of the tree routed away fifty-six targets nobody had looked at.
Eighteen mutants the tests kill were reported as survivors.

Two rules now:

- **A route trusts only what the measurement names.** A target a coverage build compiles and the measurement does not name is unmeasured, whatever the reason.
  A library's documented examples are the exception, and not through a gap: no coverage build instruments them, so no measurement is about them, and they reach by file instead.
- **A measurement that did not reach every target is not remembered.** It is sound to route by — what it could not read stays in every route — and wrong to keep, because a later run would have nothing to tell it from a whole one.

## Work a run does not have to do twice

Coverage and the instrumented baseline are measurements of a tree, not of one mutation.
**A mutation changes none of their inputs.** A run therefore files both, under separate content keys, and an unchanged rerun reads them instead of rebuilding the coverage tree or starting every baseline target again.
`cache` says how many measurement records are held and where.
`--no-cache` turns both off, because a flag that says "establish it again" has to mean both.

The baseline key is deliberately stricter than the coverage key.
It includes the complete snapshot, closure and manifests, engine and toolchain, catalog,
accepted guards and narrowing markers, target kind and harness, and the exact argument vector, working directory and effective environment of every target.
On a hit, every directly built executable must also have the same SHA-256 as the program that passed.
Only a wholly passing baseline is written; a baseline that changed the tree is not.
The document carries an integrity digest, and any missing target, out-of-catalog touch, unreadable artifact or damaged field turns the entire record into a miss.
Reuse is consequently less work, never a weaker baseline.

The claim is the one the outcome store already rests on — nothing that could change the answer changed — and it is checked the same way:
`a_remembered_measurement_routes_a_run_exactly_as_a_fresh_one_would` runs a fixture, empties the outcome store, runs it again, and holds the two reports to each other on both the verdict and the route.
A tree that *did* change is measured again, and a test says so.
The baseline has the corresponding `an_exact_passing_baseline_is_reused_without_starting_its_targets_again` test,
including changed harness arguments, a parseable damaged document, and a failing baseline that is never written.

The same reasoning is why the outcome store is keyed on the compiled closure rather than on the tree: see [upgrading](upgrading.md).

## Whether the removals are honest

Every layer here claims that running the pair would have established exactly what the run reports without it.
That is a falsifiable claim, and `crates/rust-mutants-cli/tests/toolchain_differential.rs` falsifies it: four fixtures, run twice — once with the measurement and the proofs on, once with `--no-coverage` so nothing is removed — and the two reports held to each other mutant by mutant.
A mutant the proved run never started a process for has to be one the whole run found nothing noticed.

The test refuses to be vacuous: it fails if no fixture cost less with the layers on, and it fails if no mutant was removed by a proof at all.
It takes about six seconds, which is why it runs with every other test rather than weekly.

## The layers

| Layer | Lemma | Premise | Removes |
| --- | --- | --- | --- |
| guard routing | — | no test of this target reached the mutation, or only these did | the (mutant, target) pair, or every test of it the record did not name |
| coverage routing | — | this target's measured run covered no region holding the mutation | the (mutant, target) pair |
| `branch-never-taken` | the compiler: this mutation changes nothing outside the body the condition gates | nothing of this target ran the marker at that body's first statement, or its measured run covered no region beginning inside the body | the (mutant, target) pair, and the tests of a kept target that did not enter the body |
| `never-infected` | the guard: the mutation and what it replaces are both inert, so a run may evaluate both | nothing of this target ever saw the guard's two branches answer differently | the (mutant, target) pair, and the tests of a kept target that never saw them part |
| `never-infected` | the guard: the value a return replacement overwrites is of a type whose equality is the whole of what a program can tell apart | nothing of this target ever returned a value that differed from what the replacement writes | the (mutant, target) pair, and the tests of a kept target that never returned a differing one |

`branch-never-taken` has two premises and either will do.
The instrumenter writes a marker at the first statement of every body a claim names — in the witness tree first, so the one `cargo check` that sifts the type witnesses sifts the markers too, and a body a call cannot go into (a `const` context,
a body inside a guard's own site) simply carries none.
That check caps every lint at a warning: the tree it compiles is written by this engine to ask about types, and an ordinary warning somewhere else in the workspace would otherwise stop it, leaving nothing vouched for and no way to tell a proof that was refused from one that was never made.
The marker is exact where a coverage region is inferred from where regions begin, and it needs no coverage build.
A body with no marker keeps the region as its only premise,
and the record says which markers the tree carries so that a body without one is never mistaken for a body nothing entered.

What makes a condition inert is what the sealed trait of [ADR 0008](../adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md) covers: the types whose comparison the standard library defines, and a container of one of those.
Comparing two of them runs none of the program's code, cannot panic, allocates nothing, and terminates.
A user type is refused,
because its `PartialOrd` is the program — on either side, since the two operands are asked about separately and a type of your own is in the trait neither way round.

A swap that *negates* what it replaces is offered no comparison at all.
`==` and `!=` part on every evaluation, so a record of whether they ever parted can only say that they did, and the call that recorded it would be a cost with no answer in it.
Every other swap on a reachable operator agrees somewhere.

`never-infected` has two premises as well, and the guard's is free.
A mutation on an operator the connectives of an inert condition reach —
`a < b` inside `if a < b && c`, say — leaves the condition inert: the same operands, another operator of the same class, and the compiler has already vouched for the operands through the witness tree the branch proofs use.
A guard there holds both branches, so the baseline evaluates both and records every time they part.
A target whose record never names the mutant ran a program that answered what the unmutated one answers wherever it looked, and by induction ran identically.
There is no second tree, no second run, and the question is answered per test rather than per target.

A return replacement is answered the same way and for a different reason.
The mutation writes a constant — the default, `true`, `Ok(default)`, `Some(default)` — so the guard need not evaluate it at all: it compares the value the branch that keeps the original produced against that constant.
What it may compare is the sealed `Observable` trait, which names the types whose equality is the whole of what a program can tell apart: the primitives, `str` and `String`, and `Option` or `Vec` of one of those.
Floats are outside it, because `-0.0 == 0.0` holds and `-0.0` is not what the default writes, so a probe there would call a mutation that changed the sign of a zero no change at all.
A type of your own is outside it too: a `PartialEq` that answers about one field while a test reads another would say nothing happened while a test watched the difference.

The question goes to the compiler in the witness tree, in the shape the guard will hold — `{ let v = <value>; w_default(&v); v }` — so what the compiler vouched for is literally what gets written.
A value it refuses costs the probe and never the mutant: the mutation is still measured, by running it.
And the syntax refuses first, before the compiler is asked: a value is offered a probe only when evaluating it is not itself an event, which rules out every call and every arithmetic operator.

Guard routing is the default and costs no build: the guards of the instrumented tree record which of a target's tests reached them on the run that verifies the baseline, and libtest names each test's thread after the test.
Coverage routing is the same claim established by an LLVM coverage build, kept behind `--coverage` as an independent second opinion the differential harness holds the guards to.
A run may make both, and then the guards decide the route and the regions remain the premise `branch-never-taken` rests on.

A mutation every target is removed from is not executed at all: `unreached` when the measurement placed it and nothing ran it, `discharged` when a proof took every target away.
Both are findings — the tests have a gap where the mutant is — and neither is a survivor, because nothing measured it.

## A proof without a premise removes nothing

The lemma is the compiler's or the probe's; the premise is always a measurement's.
`--no-coverage --no-touch` measures nothing, so it discharges nothing, whatever the compiler vouched for; `--no-coverage` alone still has the guards, so a probe still discharges and `branch-never-taken` — whose premise is a marker the guards record or a coverage region — is silent only where neither could be established.
A target whose profile could not be read,
or whose guards recorded nothing this run can route by, is one the measurement says nothing about: it is routed to and never discharged, because a proof resting on its silence would rest on the measurement's failure.

`prove::discharges(proof, path, covered)` is the pure function of the two.
A caller with its own coverage — `njutest` is one — discharges with its own evidence by calling it, and an audit re-implements it rather than asking the engine.

## What a run keeps

A report that names a discharge without the premises is a claim rather than a proof.
Beside its report a run writes:

| File | What it holds |
| --- | --- |
| `touched-v1.json` | which of each target's tests reached which mutation, entered which proved body, entered which item, and saw which guard's two branches part; what was reached where nothing named a test; which targets said nothing this run can route by; which mutants the tree could record anything about at all; and the catalog of items an entry names |
| `reached-v1.json` | every region each target's measured run covered, every region the build instrumented, and what the measurement could not establish |
| `catalog-v1.json` | every mutant, with the body of the branch the compiler vouched for |
| `probe/<target>.log` | what each probe process appended, when `--probe` built the tree |

An `evidence` recording names each with its size and digest.
`cargo xtask engine-audit <run>` reads them and re-decides every discharge without the engine: a target whose guards say it entered the body, or saw the two branches part, is a violation; so is one that covered a region inside the body it was discharged from; a discharge whose premises the run did not keep is unaudited, and a discharged pair the recording then executed is a violation.
The record's `narrowing` is what makes an absence in it evidence:
a mutant it does not name as compared is one `infected` says nothing about,
however often a test ran it.

## After the run

`--equivalence` asks the compiler whether each survivor's mutation is one it renders at all: the tree the user wrote is built once, the mutation is spliced in, and the two builds' executables are compared byte for byte.
An answer of `identical` says the compiler produced the same program, and the control is built again to check that the tree builds reproducibly at all — a tree whose build is not reproducible proves nothing, and one such answer withdraws every answer afterwards.

It never says `equivalent`.
Two binaries being the same bytes is a fact about what the compiler produced under the profile the tests run; whether the mutation could change behaviour is a question about the program, and a comparison of binaries does not answer it ([ADR 0013](../adr/0013-codegen-identity-is-the-equivalence-proof.md)).
A mutation the compiler refuses establishes nothing either: the question is about two programs, and there is only one.

## Where a layer is silent

`llvm-cov` regions nest.
What says a body ran is a region that *begins* inside it: the region of the function that holds the branch contains the body and says the function ran, and the region at the body's closing brace is the one the compiler emits for what follows the branch.
Reading containment as execution would discharge nothing; reading overlap as execution would discharge everything a function's own region touches.
Neither is what the measurement says.

A library's documented examples are compiled by rustdoc while cargo runs them,
so no coverage build instruments them and the engine does not start their processes itself: a mutation is routed to a documentation target by the file it is in (`doctests-routed-by-file`), which is wider than a region and is the direction a fallback must go.
The guards are silent in three more places, and each of them runs more rather than less:

- a target the engine does not start itself — a documented example, one the project configured a runner for — is not asked (`touch-not-recorded`), and every test of it stays in every route;
- a record that does not read back is refused whole (`touch-log-unreadable`),
  because lines from several processes are interleaved in it and reading the part that parsed would be a smaller wrong answer.
  A process records only into a record about the catalog it was built from, so a project whose own tests build and run instrumented trees — this engine's do — does not poison the record of the run that started them;
- a site reached on a thread nothing can name a test after — the main thread,
  a benchmark, one a test spawned — reaches **every** test of its target,
  because the record could not say which one;
- a set of tests that does not pass on its own takes its target off test routing for good, and the run notes `test-routing-unsound` with the target it was about.
