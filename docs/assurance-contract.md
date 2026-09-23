<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance contract v1

**Status: implemented.** Inherited from goatest's assurance contract v1 with the Rust toolchain in place of the Go one: the baseline, the routing and the proofs that narrow it, the mutation phase, the evidence reuse, the resources,
the repair candidates, `deep-v1`, and the verdicts.

## Meaning

An assured verdict is evidence about a named snapshot, resolved scope,
configured fault model, Rust toolchain, platform, and declared execution inputs.
It is not a proof of program correctness and does not cover faults outside that model.

`ASSURED` is reserved for a resolved `full` scope.
A changeset run that remains targeted receives `CHANGE_ASSURED`; an explicit package run receives `SCOPE_ASSURED`.
If changeset impact cannot be determined safely, the resolved scope broadens to `full`, and the report records both requested and resolved scope.

`[project] exclude` narrows the file scope and nothing else: the files it names are copied, compiled, and run like every other, and what a run is keyed on is the whole tree, so the exclusion removes findings rather than work or evidence.
A run left with no mutation to put to a test concludes `INSUFFICIENT` whatever narrowed it away, because an assurance is the claim that every mutation was noticed and a run that made none has not made it.

Replay is an operation, not a new project assurance.
It returns `REPRODUCED` when the selected finding remains observable, `RESOLVED` when it does not, and `INCONCLUSIVE` when the replay established neither.
The third is not a courtesy: an execution that never ran, that the run could not decide, or that the harness failed is one that measured nothing, and reporting it as either of the others is a claim about a measurement that was never made.
It reads no evidence, no cache, and no coverage: the finding says nothing noticed a mutation, and the way to put that to the tests again is to offer the mutation to every test of the tree rather than to the ones a measurement once said could reach it.
It advances no index and stores no verdict.

## Fault model

`standard-v1` requires:

- successful package discovery, native build of every test target, and native baseline execution;
- correct classification of every `#[test]` target, doctest suite, and custom-harness binary, setup failures, `#[ignore]` skips, and custom test-binary arguments;
- every declared resource to become ready and shut down cleanly;
- a soundness inventory of every crate — `unsafe` blocks, functions, impls and traits, foreign blocks, `static mut` — reported as evidence, with the limitation `soundness-not-executed` whenever it is non-empty ([ADR 0009](adr/0009-soundness-replaces-race.md));
- a complete `strong` rust-mutants catalog and terminal disposition for every discovered mutant; and
- stable repair-candidate validation when a candidate is produced.

`deep-v1` uses the expanded operator set and exploration limits, runs Miri on every crate with a non-empty soundness inventory and every crate that links one, and may add sanitizers.

`verified-v1` keeps the `standard-v1` mutation contract and asks one additional question about every test survivor that belongs to a deliberately closed,
pure Rust fragment.
Its `[verification]` section requires a nonzero unwind bound and a process timeout of at least one whole millisecond.
The admitted signature is a top-level free function with at least one plain by-value input built only from fixed-size primitives, arrays, tuples, and `Option`; its output is in the same equality domain without floats.
References, pointers, named types, `usize`/`isize`,
calls, globals, macros, unsafe code, configuration attributes, and profile-dependent arithmetic are rejected before a harness exists.

For an admitted survivor, njutest generates two isolated renderings and a single tagged differential assertion.
Kani never compiles the subject package:
each attempt gets a newly created dependency-free crate with a fixed manifest,
fixed lockfile, and only those two function clones plus the harness.
The full pristine source is retained as inert hexadecimal evidence, so `modelaudit` can re-mint the mutation without admitting the package's `build.rs`, procedural macros, dependencies, or unrelated modules as proof inputs.
Only that assertion failing while every safety and unwind property succeeds is `model-noticed`; only every property succeeding is `model-proved`.
An exhausted unwind bound, wall-clock cutoff,
process failure, unknown property, tree drift, or protocol mismatch is one typed `undecided` record and leaves the mutation survived.
It can never be promoted by a summary count or an exit code alone.

The isolated crate always runs with Cargo networking disabled.
Kani 0.68 does not accept Cargo's `--locked` option on either its proof or harness-list command, so both phases are instead guarded by the exact dependency-free manifest, exact empty lockfile, offline environment, and a whole-crate check before and after every subprocess.
Every report carries a closed `crate_input` identity whose domain-separated digest binds those fixed files and the closed `minimal-v1` subprocess environment to the retained generated source; deserialization relates it to the rendered-source digest,
and `modelaudit` recomputes it independently from the artifact bytes.

The verifier boundary is Kani 0.68 with its exact exported schema, compiler,
CBMC/goto, solver, target, and build-mode identity retained alongside the generated Rust, raw result, process termination, and their digests.
This is a proof under that pinned verifier compiler's semantics, not a claim that its compiler is byte-identical to the compiler that ran the tests.
`modelaudit` reconstructs the generated source and re-parses the raw result independently;
`proofaudit` requires exactly one model record for every `verified-v1` survivor.
The remaining trusted boundary is the local `cargo-kani` executable,
versioned Kani/CBMC bundle, Cargo/rustup proxy, operating system, and filesystem isolation, not a human interpretation of checker output.
Absolute/no-follow paths, a rebuilt minimal environment, the exact version banner, and exported compiler/backend identity narrow that boundary, but the report does not carry cryptographic digests of every host executable or claim to defeat a hostile administrator replacing one between validation and execution.
CI constructs the boundary with a locked install of exactly `kani-verifier =0.68.0`.

Benchmarks are not part of either correctness contract.
Doctests are run and classified as one target per library, and mutations are routed to them: a mutation only a documented example can notice is noticed by it rather than reported as surviving.
What such a target carries is not coverage but the files its library is made of, so it reaches every mutation in them and narrows none of them, which `doctests-routed-by-file` says.

One target per library rather than one per example is not a simplification.
rustdoc merges a file's examples into one compilation, and asking that harness for one of them by name runs every example in the file — a filter that matches nothing filters everything out, and a filter that matches one example runs all of them.
So a kill a documented example finds is attributed to the library's documentation, and `--test-runtool` is what would attribute it to the example.

A library that documents no example has nothing to run and is not a target: a target that ran nothing would raise a finding about documentation nobody wrote.
A test binary that brings its own harness cannot be asked for one of its tests and is measured whole, which `custom-harness` says.

A mutation is measured against a suite that passes.
A run whose baseline saw a target fail reports that and measures no mutation: there is nothing for a mutation to change about a test that was going to fail anyway.
A future performance contract must be explicit rather than treating ordinary benchmarks as tests.

## Mutation routing

A target is a test binary: a library's own tests, one integration test, a binary's own tests, an example, a procedural macro crate's own tests, or a library's documented examples.
A route names the targets that could notice a mutation and, for each of them, which of its tests the mutation is put to.
The tests belong to the target rather than sitting beside it, so a route cannot name tests of a target it does not keep, and cannot keep a target whose tests it forgot to say anything about.

Reach is decided by the guards the instrumented build compiled in, and the one run of every target that establishes the baseline is the run that records it.
Each guard appends the site it evaluated under the name of the thread it evaluated it on, libtest names a test's thread after the test, and what comes back is therefore per test rather than per target.
Nothing is measured twice because nothing is measured a second time at all: the measurement is the baseline.

That makes one premise carry every route: what a target reaches is a function of the target, and not of the order, the clock, or what an earlier process of the run left behind.
The run checks it where it already runs a target a second time.
The original-code control that confirms a kill runs the whole target again under the conditions the baseline ran under — the same arguments, the same environment, a fresh temporary directory of its own — and its guards record too.
Where it passed exactly the tests the baseline passed and the target's union of sites reached, bodies entered, or sites infected differs, the baseline record is one sample rather than a measurement, and the report raises `unstable-baseline` about the target, counting the `unreached` claims and the discharged executions that rest on it.
One such observation is enough; a counterexample does not wait for a second.
A measured target no comparable control recorded is named by `drift-not-measured`, because a proof read off it rests on one run.
This release reports a moved target and does not yet run again what rested on it ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)).

The decision widens whenever the evidence cannot carry it, and every widening runs more rather than less.
The route names which one it was:

| Fallback | What could not be established |
| --- | --- |
| `not-measured` | nothing was recorded at all, so every test of every target runs |
| `position-unknown` | the catalog could not say where the mutation is |
| `outside-blocks` | no instrumented region contains the position |
| `coverage-incomplete` | a target the measurement could have named carries no profile |
| `touch-incomplete` | a target's guards recorded nothing this run can route by |

A target the record does not name is one nothing was established about, and it stays in the route with every test of it.
A site recorded on a thread nothing names a test after reaches every test of its target, for the same reason.
Being absent from a measurement is not the same as being measured and reaching nothing, and reading the first as the second turns a kill into a survivor.

Saying that a position reaches nothing is a claim about the code rather than a gap in the measurement, and it holds only for a target that was measured, was asked, and answered that nothing of it reached the site.
A mutation no such target reached is `unreached`: nothing runs, and the finding says no test executes this code.
The route names every target that was in a position to notice and did not.
Naming them rather than counting them is what lets `xtask proofaudit` re-derive the layer rather than confirm that the engine said it.

A library's documented examples are not among the targets a measurement names.
rustdoc compiles them while cargo runs them, so no instrumented build reaches them — they are not unmeasured targets, they are targets this measurement is not about — and they reach a mutation when it is in a file their library is made of.
`doctests-routed-by-file` says so, on every report where one ran.

`unreached` is not `surviving`; both are `survived` in the mutant inventory,
so the accounting equations below are unaffected by how a mutant was routed.

The rule itself lives in the engine, and this runner asks it rather than keeping one of its own.
Two rules for one question disagree eventually, and the disagreement this pair would make is a kill reported as a survivor.

### Discharging a test a branch proof rules out

A reaching set the measurement decided is narrowed once more where the mutation itself carries a proof.
rust-mutants publishes one for an edit that can only make the condition of an `if` or a `while` less often true, and it names the span of the body that condition gates, from its opening brace to its closing brace.

Write C for the original condition and C′ for the mutated one.
C′ implies C,
and the whole condition is inert — no effects, no possible panic, guaranteed to terminate, which the engine establishes with the compiler's help ([ADR 0008](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)).
So a target during which no statement of the gated body ran evaluated C to false every time it was evaluated, evaluated C′ to false there too, took the same branch on every evaluation, and ran identically on the two programs.
It cannot have observed the mutation.
Such a target is *discharged*: removed from the reaching set without being executed, and named in the route's `discharged` beside the proof that removed it.
A discharge changes what a run pays, never what it concludes.

The narrowing applies only where the evidence carries it.
It is attempted on a route the measurement decided with no fallback, and never on one that widened.
It requires that the instrumenter wrote the body's marker at all, because otherwise no target's silence about the body means anything.
A fuzz target is never discharged: it explores inputs beyond the corpus its measurement was taken on.

A mutant every reaching target was discharged for is resolved without a single execution, and reported as a `surviving-mutant`.
That no test takes the branch the mutation narrows is the finding — a real gap in the suite, stated for the cost of reading a record the baseline already wrote.

### Discharging a test that never saw the mutation make a difference

A reaching set the measurement decided is narrowed a second time by what the guards recorded.
Write reaching for the routing decision as a whole:

```text
reaching(m, t) = touched(m, t)
               ∧ ¬branch-discharged(m, t)
               ∧ ¬(compared(m) ∧ measured(t) ∧ m ∉ infected(t))
```

The guard the instrumented tree carries for a mutation has two sides: the one the mutant takes when it is active, and the one the original takes when it is not.
For a mutation the engine has a question for, the second side compares the value the original computed at the mutated site against the one the mutant would have put there, and records the site when the two differ.
Nothing is built or run for this.
The run that establishes the baseline is the run that records it, and it is the same run that records which test reached which site:
one execution answers both questions, because the questions are about the same moment.

The engine runs a test binary whole, so what it records is that some test of a binary saw the site differ and not which one; a binary that never saw it differ is one whose every test ran the original program and the mutated one through identical states, and none of them can have observed the mutation.
Such a target is *discharged* with reason `never-infected`.
The coarseness costs discharges and never soundness: one test of a binary seeing the site differ keeps every test of that binary.

That the recording is a proof is the engine's obligation.
A mutant does not evaluate the operand it replaced, so a question is attached only where leaving that operand unevaluated changes nothing observable — every operand of the statement is effect-free — where the recorded comparison is always reached —
the replaced operand cannot panic — and where equal values mean equal behaviour.
That last one is the narrowest, and it is the compiler that enforces it: the comparison is written against a sealed trait, and a type outside it does not compile.
What is inside is the integers, `bool`, `char`, the unit,
`str`, `String`, `Option<T>`, `Vec<T>`, and a reference to any of them — the types whose equality is the whole of what a program can tell apart.
A float is refused because `-0.0 == 0.0` holds and `-0.0` is not what the default writes;
an equality that answers about one field while a test reads another is refused because it would otherwise say a test saw nothing while it watched the difference, and the discharge would remove the test that finds the defect.

The question is put to the compiler before it is written into the tree that has to build, on the pass that already asks whether each mutation is accepted.
A site the compiler will not vouch for loses its question and keeps its mutation:
the mutant is executed and measured like any other.
njutest states nothing about a site the engine did not claim, and holds what the engine does claim to the recorded kills of every dogfood run through the offline `proofaudit` infection layer.

The narrowing applies only where the measurement carries it, and everything else is kept.
A mutation the instrumented tree carries no comparison for —
`compared` is false — is absent from every measurement there will ever be, so its absence from one says nothing.
A target the guards did not record carries no facts at all.
Both proofs may answer for targets of the same route; they are applied in order — branch first, then infection.

The infection layer narrows and never widens.
It records per binary, so what it holds is that some test of a binary saw the mutation make a difference and not which one: putting a target back on the strength of that would put every test of the binary back, whatever the record of each said.
The case it would be for — a measurement silent about work the tests do — is the one a fallback already answers, and a fallback runs every test of every target, which is more than the widening would.

All three narrowings are proof layers in the sense of [ADR 0004](adr/0004-proof-layers-not-budgets.md): an execution is removed only where evidence the run already holds proves it could not observe the mutant,
never by a time budget, a sample, or an exclusion of slow targets, and a layer that cannot establish its premise keeps the execution.

### Proving that nothing could have noticed

A mutation nothing noticed is a gap in the tests unless there was nothing to notice.
`[mutation] equivalence` asks the compiler about every one of them:
the tree is built, built again with the mutation spliced in, and the executables the two builds produced are compared byte for byte.
Identical executables are the same programs, and the same program makes the same observations, so no test can tell the two apart.
The argument needs neither a deterministic compiler nor a correct one ([ADR 0013](adr/0013-codegen-identity-is-the-equivalence-proof.md)).

Identical is what the engine says.
`equivalent` is what this run says, and only where five premises hold: a control still builds the original to the bytes it built to, no test wrote into the tree while it was being measured,
the package holds no `unsafe`, the route was decided by the measurement with no fallback and named at least one target, and no killer was recorded.
The fourth carries the layer.
A mutation of a function no test calls is dropped by the linker and the artifacts come out identical for the opposite of a reassuring reason, so a mutation nothing reached keeps its finding whatever the compiler did with it.

The comparison is made under the project's own test profile, because that is the profile the tests run under: `x + 0` and `x - 0` are the same instructions at `opt-level = 1` and different ones at the `opt-level = 0` cargo gives a test profile by default.
On a project that leaves it at the default the layer proves almost nothing, and says so rather than appearing to have looked.

This removes findings and never executions.
Every test that reaches the mutation has already run by the time the layer does, and turning the layer off leaves the finding in place.

## Mutation confirmation

A mutant is `killed` only after:

1. an initial mutant execution fails;
2. the original-code control for that request passes; and
3. a second execution of the same mutant/request also fails.

Each distinct control command — the target binary, arguments, and environment of the killing request — runs once per mutation phase and its outcome answers every kill that shares it.
A request that named no target, because the package suite is what settled the mutation, is narrowed to the target the suite found the answer in: the control and the second execution are about the test that found the kill, not about the suite around it.
The snapshot is frozen for the whole phase and re-verified afterwards.

A target with no tests in it — a library or a binary whose harness is empty —
answers neither question, and a suite request passes over it.
Reading its silence as an answer would make a mutation inconclusive because a sibling target had nothing to run.

An original control failure is `flaky-mutation-control`; a non-reproducing kill is `flaky-mutation-kill`.
Both are inconclusive evidence and prevent an assured verdict.
A mutation that does not compile is `compile-rejected`, never "compile-equivalent".

Every cataloged mutant has exactly one report-v1 disposition, and the columns say what the records say:

```text
cataloged      = rejected + executed + unreached + equivalent
executed      >= killed + survived + step_limit_reached + waited
accepted      <= survived + unreached + equivalent
reused_killed <= killed        reused_survived <= survived
```

`equivalent` is a column of its own rather than a part of `survived`, because a reader who cannot tell "nobody noticed this" from "nobody could have" cannot act on either.

`executed` is an inequality because a pair that did not agree and a harness that could not run are executions that established neither a kill nor a survival.
The aggregate counts must match the ID-level mutant inventory exactly, and `cargo xtask proofaudit` re-derives every one of these from the recording rather than asking the runner whether it agrees with itself.

An acceptance answers for a mutation nothing noticed — one every reaching test passed and one no measured test reaches alike — and for nothing else.
An outcome that established nothing either way is not a decision anybody can sign off, so a pair that did not agree, a harness that could not run, and a budget that expired keep their findings whatever a reviewer wrote.

### Reusing a verdict an earlier run reached

The reasoning is [ADR 0007](adr/0007-survived-evidence-is-universal.md).

A full run — the whole project, in a first round no repair has modified —
records what it established about every mutant it can state a checkable claim for, and the next such run resolves those mutants from the records instead of executing them.
A kill is an existential claim and a survival is the universal one; the two are reused under conditions of the same shape, over one target and over every target respectively.

A believed record is an execution that did not happen, so this is a layer and [ADR 0004](adr/0004-proof-layers-not-budgets.md) decision 4 asks the same of it as of any other: the route of every mutant records either the run whose answer it took or why it took none — `nothing-recorded`, `unreadable`,
`target-unknown`, `not-routed`, `key-changed`, `not-passing`,
`target-entered`, `nothing-routed` — and never both.
A run that kept no store of earlier answers records neither, which is what parts it from a run whose store refuses everything: told only how long the two took, nobody can tell them apart.

#### A kill

Reused when: the mutant has the same content-addressed identity; the recorded killer is a target this run's own coverage still routes to the mutant, after every discharge; that target has the same behaviour key; and this run's own baseline ran that target on the original tree and saw it pass.

#### A survival

Reused when the mutant has the same identity and **every** target this run's coverage routes to it, after every discharge, is one of the recorded targets with the same key, seen to pass by this run's baseline.
A reaching set smaller than the recorded one is still covered; a target that entered it is a test nothing was ever run against.
Fuzz targets disqualify a survival in both directions.

#### A mutant the evidence cannot say nothing reaches

Settled by running the targets the route widened to, so the claim is recorded as the conjunction of those targets' own behaviour keys: a target that enters or leaves the route refuses reuse where one key over the package would have hidden it.
A record this run cannot resolve to the targets its own baseline saw pass is neither believed nor written, because half of a set is a smaller claim wearing the same name.
A mutant the premise of `unreached` holds for is a claim about the code and is reused by nothing.

#### A bound that expired

A bound that expired is not a proof about the mutant.
Reusing one keeps a finding and never removes one.
Each mutation command, and the probe command that measures the same target, gets five times its measured baseline duration plus five seconds, with a 30-second floor; the contract caps calibration at 30 minutes for `standard-v1` and five hours for `deep-v1`, and `[execution].timeout` is a further upper bound.

An expired budget buys one measurement with the machine to itself.
The budget is derived from a duration the baseline measured, and a duration measured while other test processes were running is a fact about the load as much as about the mutation, so before a run decides that time really ran out it stops starting anything else and measures once more.
What that measurement observes is what stands: a mutation that completes under it was observed completing,
and only a budget that expires again with nothing else running is `waited`.
`step-limit-reached` is not this and buys no quiet measurement: a verified guard notice already makes the execution boundary deterministic.
It remains a non-verdict, however.
The count proves where this execution was stopped, not that the mutation caused divergence; a finite control execution can cross the same boundary.
It is never detection, survival, score, cache evidence, or an acceptance candidate.
This is not a retry policy — one expired budget buys exactly one quiet measurement, and the recording says of every execution whether the machine was given to it.

An expired budget remains inconclusive under every bound.
The record names the target time ran out under as the **last** of its executed targets, stored in execution order.
`njutest replay <finding-id>` bypasses evidence entirely, which is how a `waited` is deliberately re-run.

#### The behaviour key

The behaviour key is an allowlist over what the run already digested for its own snapshot identity: every source file of the crates the target's test binary links (the package's own tests included, dependencies' tests excluded),
the files beside those crates that `include_str!`, `include_bytes!`, and a build script's `rerun-if-changed` name, the manifests, the `Cargo.lock` checksums, the toolchain, the platform, the selected environment, the contract, the test arguments, the features, both timeouts, the njutest and rust-mutants versions, and a fuzz target's corpus.
Diagnostics — tracing,
kept temporaries — and parallelism are outside every key.

A run measures up to `[execution] jobs` targets at once in the baseline and up to that many mutations at once after it — the processors the machine offers,
capped at four, when the configuration does not say, and one whenever a resource only one test may hold at a time is configured.
The two are measured the same way on purpose: a mutation's budget is derived from what the baseline measured of the same target, and a duration taken alone is not the one a target running beside three others will take.
Measuring two mutations at once is not a budget: every mutation still runs, against every test its route named, and nothing is sampled or skipped.
Workers commit nothing; the answers are put back in the order the catalog has them, so what a report says is the same however the processors were shared out.
Each execution is given a temporary directory of its own, so two of them cannot meet in one another's files.

A package whose sources use a directory-reading API — `std::fs::read_dir`,
`walkdir`, `glob`, `globset`, `ignore`, `include_dir!`, or the working directory — keys the whole snapshot for every target it links and for its suite.
Rust offers no execution observation of file reads that is portable and unprivileged, so the selection is static and widens rather than trusts.
A package with a build script whose `rerun-if-changed` cannot be read keys the whole tree as well.
Nothing is excluded from testing or reuse by name.

Two kinds of kill are neither recorded nor believed: a kill fuzzing found, and a kill by a batch that does not name the killer.
Reuse is confined to a first round, the whole project, no configured resources, and no replay.
Nothing expires a record; a stale record is removed by being contradicted.

A reused verdict is one of the executed dispositions: `reused_killed + reused_survived <= executed`, each carries the `provenance` of the run that observed it, and its route in the trace records the reuse with no execution beside it.
A reused verdict raises its finding again through the acceptances of the run reading it.

## Acceptances

An acceptance is human authorization, not a mutation result.
It requires a finding ID and a non-empty reason, and may carry an RFC3339 expiry, an owner and a ticket; `njutest accept` writes all of them.
Every mutation marked `accepted` must reference a matching record.
An acceptance whose expiry has passed answers for nothing, and the findings it was hiding are raised again:
the expiry is when the reviewer said to look again, and a run that read it as a comment would go on exempting a mutation on the strength of a decision its author had already put an end to.
One that names no date never lapses, which is what a reviewer who wrote none asked for.

`njutest review` goes through one run's gaps a place at a time, drawn the way the run drew them, and prints the `njutest accept` lines for whatever the reviewer decided.
It prints rather than writes: a review is somebody deciding, and a change to their project is a separate act they make with the command every other surface already hands them.

What the run **established nothing about** — a bound that expired, a harness that never started — is shown and cannot be accepted.
An acceptance says a reviewer looked at what a run found and decided it may stand; where nothing was found there is nothing to have looked at, and recording one would be a decision about a measurement that never happened.
The loop asks a different question there, with an answer type that has no such arm, so the wrong acceptance is not a thing the program can express ([ADR 0023](adr/0023-a-run-may-not-conclude-from-how-it-measured.md)).

## Parts of one catalog

`njutest verify --shard K/N` divides the judging.
Every part measures the whole baseline, because a mutation cannot be judged against tests that were not run;
what a shard divides is which mutations are put to those tests.
The rule is the engine's, so both products cut a catalog the same way: the dense catalog index modulo N, counting K from one.
Two runs of the same tree therefore agree about which part holds which mutation without saying a word to each other, and every mutation belongs to exactly one part — nothing is sampled, nothing is skipped,
and no execution is paid for twice.

That is what keeps dividing the work out of [ADR 0004](adr/0004-proof-layers-not-budgets.md)'s way.
A budget decides not to run something; this decides which machine runs it.
The parts balance by count rather than by cost, so a part holding a slow mutation takes longer, and how long is something to measure rather than to predict.

A part concludes `PARTIAL` and records its shard.
It assures nothing on its own:
the mutations it did not judge are not mutations nothing noticed, they are mutations nobody put to a test, and a report that called that an assurance would be claiming the one thing it did not look at.
A finding in a part is a finding,
so a part that found a defect says `DEFECT`.

`njutest merge <REPORT>…` writes the report the whole would have written.
It passes one already unsharded report through, or requires exactly one report for every label `1/N` through `N/N`.
It refuses no parts at all; a missing,
repeated, malformed, mixed unsharded, or differently divided part; parts that disagree about the tree, configuration, contract, effective scope, or runner and engine versions; and two parts that both judged one mutation.
Only a complete set is allowed to become an unsharded report: an absent shard has no mutant row with which to overlap, so disjoint rows alone cannot prove that the union is the whole.
These refusals are this runner's and not the engine's.
A run report is a collection of what running something said,
and two collections add up whatever produced them; an assurance report is one claim, that a contract was met, and a claim assembled from a part that met it and a part that met something else is true of neither.

The mutant rows of the whole are the union of the parts'.
Its accounting is derived from that union rather than added up from what each part counted, so the whole's columns say what the whole's own records say.
The exception is `accepted`, which is a fact about a reviewer rather than about a mutation and appears in no record, and is therefore summed — each mutation belongs to one part, so each acceptance is counted once.
No score crosses a merge at all: two ratios over different denominators average into a number no run observed.

Every run's identity carries its shard, so a part never reads back the whole's stored answer and a whole never reads back a part's.

## What a specification says

`njutest spec [SUBJECT]` reads one stored run and lists, for every item the subject names, each change the run made inside it and where that change stands.
It runs nothing and establishes nothing: every line is a projection of the report.

| Section | What it means | The decisions it holds |
| --- | --- | --- |
| what is pinned | every build noticed the change or found it to be the same program, and at least one noticed it: a target's tests failed on it, the compiler refused it, or the model checker found an input that tells the two apart | `killed`, `compile-rejected`, `model-noticed` |
| what is left free | some build noticed nothing of a change that makes a different program there, and every build established something | `survived`, `unreached` |
| what is the same program | every build found the change to be the same program | `equivalent`, `model-proved` |
| what the run could not tell | some build established nothing about it | `step-limit-reached`, `waited`, `unconfirmed`, `errored` |

A change is listed under the section its builds decided together, which is the same lattice minimum the verdict reads, so a change one build noticed and another did not is free, and a change that waited in any build is in the last section.
What each build established is on the lines beneath it.
An attempt that established nothing is in neither of the first two sections: it is not a chance the tests were given and did not take.

Each line says only what the run established.

- A kill names the target that noticed, the targets asked before it with what each answered, and how many targets reach the change and were never asked.
  It never says *only*: the mutation phase stops at the first target that notices, so that target is the first in route order and not a distinguished one ([ADR 0023](adr/0023-a-run-may-not-conclude-from-how-it-measured.md)).
- A free change a proof removed every target of names the proof and says to check the proof rather than to write a test ([ADR 0004](adr/0004-proof-layers-not-budgets.md)).
- A change nothing executes says so, which is a different gap from one the tests ran and did not notice.
- An answer read back from an earlier run names that run and claims nothing about who else it asked, because the route beside it is this run's and the answer is the earlier one's.
  An answer inherited from a checkpoint without a route says the record does not say what ran it.
- A change a reviewer accepted says so.
- A change left free that rests on a target whose baseline reach moved on a control says that target's reach is not a measurement: the route kept the target off the change, and ADR 0025 found that what the target reaches is not a function of it.
  The rule is `report::drift::rests_on`, the one the `unstable-baseline` finding counts with, so the page and the finding cannot disagree about which changes rest on a move.

A subject is a file, written whole or by its last components (`src/lib.rs`, `lib.rs`); an item as the source names it (`retry`, `Baseline::retry`), which also names every item inside it; or `PATH:ITEM`.
An item is matched segment by segment, so `retry` does not name `retry_all`, and every item that matches is listed under its own path rather than merged with the others.
A subject the run made no change in is refused with `NJ6006`, naming the run and how much of the workspace it asked about, rather than drawn as an item with nothing pinned and nothing free.
A part's report is refused as every command that needs a complete report refuses it: it holds a slice of the catalog, so what it says an item pins and leaves free would not be the item's.
Merge the parts first.
The command exits 0 whatever the specification says, because it describes and does not judge.

## DEFECT, INSUFFICIENT, and ERROR

`DEFECT` means user code violated a baseline, soundness, build, or test contract.
`INSUFFICIENT` means execution completed but a survivor, flaky or inconclusive outcome, unpersisted fuzz kill, excluded boundary, unsupported Miri operation, or other evidence gap remains.
`ERROR` covers incomplete accounting and toolchain, provider, filesystem, protocol, or workspace failures.

A limitation is always structured with a stable code.
Excludes, estimates,
unavailable metadata, and skipped later phases must never be hidden behind an assured-looking percentage.
