<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Assurance contract v1

**Status: implemented.** Inherited from goatest's assurance contract v1 with
the Rust toolchain in place of the Go one: the baseline, the routing and the
proofs that narrow it, the mutation phase, the evidence reuse, the resources,
the repair candidates, `deep-v1`, and the verdicts.

## Meaning

An assured verdict is evidence about a named snapshot, resolved scope,
configured fault model, Rust toolchain, platform, and declared execution
inputs. It is not a proof of program correctness and does not cover faults
outside that model.

`ASSURED` is reserved for a resolved `full` scope. A changeset run that remains
targeted receives `CHANGE_ASSURED`; an explicit package run receives
`SCOPE_ASSURED`. If changeset impact cannot be determined safely, the resolved
scope broadens to `full`, and the report records both requested and resolved
scope.

Replay is an operation, not a new project assurance. It returns `REPRODUCED`
when the selected finding remains observable or `RESOLVED` when it does not.
It reads no evidence, no cache, and no coverage: the finding says nothing
noticed a mutation, and the way to put that to the tests again is to offer the
mutation to every test of the tree rather than to the ones a measurement once
said could reach it. It advances no index and stores no verdict.

## Fault model

`standard-v1` requires:

- successful package discovery, native build of every test target, and
  native baseline execution;
- correct classification of every `#[test]` target, doctest suite, and
  custom-harness binary, setup failures, `#[ignore]` skips, and custom
  test-binary arguments;
- every declared resource to become ready and shut down cleanly;
- a soundness inventory of every crate — `unsafe` blocks, functions, impls
  and traits, foreign blocks, `static mut` — reported as evidence, with the
  limitation `soundness-not-executed` whenever it is non-empty
  ([ADR 0009](adr/0009-soundness-replaces-race.md));
- a complete `strong` rust-mutants catalog and terminal disposition for every
  discovered mutant; and
- stable repair-candidate validation when a candidate is produced.

`deep-v1` uses the expanded operator set and exploration limits, runs Miri on
every crate with a non-empty soundness inventory and every crate that links
one, and may add sanitizers.

Benchmarks are not part of either correctness contract. Doctests are run and
classified as one target per library but carry no coverage and route no
mutant; the limitation `doctests-not-routed` says so. A library that documents
no example has nothing to run and is not a target: a target that ran nothing
would raise a finding about documentation nobody wrote. A test binary that
brings its own harness cannot be asked for one of its tests and is measured
whole, which `custom-harness-whole-binary` says. A future performance
contract must be explicit rather than treating ordinary benchmarks as tests.

## Mutation routing

A mutant is run by the measured targets that reach it. Reach is decided by the
coverage regions of the baseline profiles and the start position — line and
column, in the unit [report v1](report-v1.md) fixes — the catalog reports for
the mutation: a target reaches the mutant when one of the regions it executed
contains that position.

The decision gives way to the whole file whenever the evidence cannot carry it.
A mutant with no reported position, and a position that lies in a gap between
the regions the coverage toolchain cut, are both routed by every target that
executed the file. A target restored from a checkpoint carries no regions and
keeps reaching its whole file.

Saying that a position reaches nothing is a claim about the code rather than a
gap in the measurement, and it rests on two premises: that instrumentation
described the position, so a target's silence about it is a fact, and that
every target routing reads carries coverage, so its silence is readable. A
position both premises hold for, and no such target executed, is `unreached`:
nothing runs, and the finding says no test executes this code. A library's
documentation is not among the targets routing reads — it carries no coverage
by construction rather than by accident, and `doctests-not-routed` says so.

Where a premise fails there is no proof, and the fallback is toward running
more. The package suite — every target the run prepared, in one execution —
settles the mutation instead, and the route says which premise failed:
`position-unknown` where the catalog could not say where the mutation is,
`outside-blocks` where no instrumented region contains the position, and
`coverage-incomplete` where a measured target carries no coverage at all.
The suite's answer is an ordinary kill or survival, so no accounting column
holds a mutation whose disposition rests on an absence of evidence.

`unreached` is not `surviving`; both are `survived` in the mutant inventory,
so the accounting equations below are unaffected by how a mutant was routed.

### Discharging a test a branch proof rules out

A reaching set decided by region is narrowed once more where the mutation
itself carries a proof. rust-mutants publishes one for an edit that can only
make the condition of an `if` or a `while` less often true, and it names the
span of the body that condition gates, from its opening brace to its closing
brace.

Write C for the original condition and C′ for the mutated one. C′ implies C,
and the whole condition is inert — no effects, no possible panic, guaranteed
to terminate, which the engine establishes with the compiler's help
([ADR 0008](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)).
So a target during which no statement of the gated body ran evaluated C to
false every time it was evaluated, evaluated C′ to false there too, took the
same branch on every evaluation, and ran identically on the two programs. It
cannot have observed the mutation. Such a target is *discharged*: removed from
the reaching set without being executed, and named in the route's
`discharged` beside the proof that removed it. A discharge changes what a run
pays, never what it concludes.

The narrowing applies only where the evidence carries it. It is attempted on a
route decided by region with no fallback, and never on one decided by file. It
requires that the body was instrumented at all — some instrumented region must
begin inside the span — because otherwise no target's silence about the body
means anything. A fuzz target is never discharged: it explores inputs beyond
the corpus its coverage was measured on. Neither is a target restored from a
checkpoint, which carries no regions to argue with.

A mutant every reaching target was discharged for is resolved without a single
execution, and reported as a `surviving-mutant`. That no test takes the branch
the mutation narrows is the finding — a real gap in the suite, stated for the
cost of reading a coverage profile.

### Discharging a test the probe pass shows cannot observe the mutation

A reaching set decided by region is narrowed a second time by what the probe
pass measured. Write reaching for the routing decision as a whole:

```text
reaching(m, t) = covered-region(m, t)
               ∧ ¬branch-discharged(m, t)
               ∧ ¬(probed(m) ∧ measured(t) ∧ m ∉ infected(t))
```

The probe tree is the program the user wrote, with no mutant ever active. For
each mutation the engine has a probe form of, that tree records — without
effects of its own — whether the value the original computed at the mutated
site ever differed from the constant the mutant would put there. The engine
runs a test binary whole and records what that binary infected, so what the
pass measures is a binary rather than one of its tests; a binary that never saw
the site differ is one whose every test never saw it differ, and each of that
binary's targets ran the original program and the mutated one through identical
states. None of them can have observed the mutation. Such a target is
*discharged* with reason `never-infected`. The coarseness costs discharges and
never soundness: one test of a binary seeing the site differ keeps every test of
that binary.

That the recording is a proof is the engine's obligation. A mutant does not
evaluate the operand it replaced, so rust-mutants attaches a probe form only
where leaving that operand unevaluated changes nothing observable — every
operand of the statement is effect-free — where the recorded comparison is
always reached — the replaced operand cannot panic — and where equal values
mean equal behaviour. That last one is the narrowest. A probe reads `==` as the
answer to whether a test could have seen the replacement, so it is stated only
for the types whose equality is the whole of what a program can tell apart: the
integers, `bool`, `char`, and the unit. The compiler itself refuses every other
probe. A float is refused because `-0.0 == 0.0` holds and `-0.0` is not what
the default writes; an equality that answers about one field while a test reads
another is refused because the probe would otherwise say a test saw nothing
while it watched the difference, and the discharge would remove the test that
finds the defect. mjutest states nothing about a site the engine did not claim,
and holds what the engine does claim to the recorded kills of every dogfood
run through the offline `proofaudit` infection layer.

The narrowing applies only where the measurement carries it, and everything
else is kept. A mutant the engine compiled no probe form for — `probed` is
false — is absent from every measurement there will ever be, so its absence
from one says nothing. A target the pass did not measure carries no facts at
all. Both proofs may answer for targets of the same route; they are applied in
order — branch first, then infection.

All three narrowings are proof layers in the sense of
[ADR 0004](adr/0004-proof-layers-not-budgets.md): an execution is removed only
where evidence the run already holds proves it could not observe the mutant,
never by a time budget, a sample, or an exclusion of slow targets, and a layer
that cannot establish its premise keeps the execution.

## Mutation confirmation

A mutant is `killed` only after:

1. an initial mutant execution fails;
2. the original-code control for that request passes; and
3. a second execution of the same mutant/request also fails.

Each distinct control command — the target binary, arguments, and environment
of the killing request — runs once per mutation phase and its outcome answers
every kill that shares it. A request that named no target, because the package
suite is what settled the mutation, is narrowed to the target the suite found
the answer in: the control and the second execution are about the test that
found the kill, not about the suite around it. The snapshot is frozen for the
whole phase and re-verified afterwards.

A target with no tests in it — a library or a binary whose harness is empty —
answers neither question, and a suite request passes over it. Reading its
silence as an answer would make a mutation inconclusive because a sibling
target had nothing to run.

An original control failure is `flaky-mutation-control`; a non-reproducing
kill is `flaky-mutation-kill`. Both are inconclusive evidence and prevent an
assured verdict. A mutation that does not compile is `compile-rejected`, never
"compile-equivalent".

Every cataloged mutant has exactly one report-v1 disposition, and the columns
say what the records say:

```text
cataloged      = rejected + executed + unreached
executed      >= killed + survived + timed_out
accepted      <= survived + unreached
reused_killed <= killed        reused_survived <= survived
```

`executed` is an inequality because a pair that did not agree and a harness
that could not run are executions that established neither a kill nor a
survival. The aggregate counts must match the ID-level mutant inventory
exactly, and `cargo xtask proofaudit` re-derives every one of these from the
recording rather than asking the runner whether it agrees with itself.

An acceptance answers for a mutation nothing noticed — one every reaching test
passed and one no measured test reaches alike — and for nothing else. An
outcome that established nothing either way is not a decision anybody can sign
off, so a pair that did not agree, a harness that could not run, and a budget
that expired keep their findings whatever a reviewer wrote.

### Reusing a verdict an earlier run reached

The reasoning is [ADR 0007](adr/0007-survived-evidence-is-universal.md).

A full run — the whole project, in a first round no repair has modified —
records what it established about every mutant it can state a checkable claim
for, and the next such run resolves those mutants from the records instead of
executing them. A kill is an existential claim and a survival is the
universal one; the two are reused under conditions of the same shape, over
one target and over every target respectively.

#### A kill

Reused when: the mutant has the same content-addressed identity; the recorded
killer is a target this run's own coverage still routes to the mutant, after
every discharge; that target has the same behaviour key; and this run's own
baseline ran that target on the original tree and saw it pass.

#### A survival

Reused when the mutant has the same identity and **every** target this run's
coverage routes to it, after every discharge, is one of the recorded targets
with the same key, seen to pass by this run's baseline. A reaching set smaller
than the recorded one is still covered; a target that entered it is a test
nothing was ever run against. Fuzz targets and targets restored from a
checkpoint disqualify a survival in both directions.

#### A mutant the evidence cannot say nothing reaches

Settled by running the package suite, which runs every prepared target, so the
claim is recorded as the conjunction of those targets' own behaviour keys: a
target that enters or leaves the suite refuses reuse where one key over the
package would have hidden it. A mutant both premises of `unreached` hold for
is a claim about the code and is reused by nothing.

#### A timeout

A timeout is not a proof about the mutant. Reusing one keeps a finding and
never removes one. Each mutation command, and the probe command that measures
the same target, gets five times its measured baseline duration plus five
seconds, with a 30-second floor; the contract caps calibration at 30 minutes
for `standard-v1` and five hours for `deep-v1`, and `[execution].timeout` is a
further upper bound. An expired budget remains inconclusive under every bound.
The record names the target time ran out under as the **last** of its executed
targets, stored in execution order. `mjutest replay <finding-id>` bypasses
evidence entirely, which is how a timeout is deliberately re-run.

#### The behaviour key

The behaviour key is an allowlist over what the run already digested for its
own snapshot identity: every source file of the crates the target's test
binary links (the package's own tests included, dependencies' tests excluded),
the files beside those crates that `include_str!`, `include_bytes!`, and a
build script's `rerun-if-changed` name, the manifests, the `Cargo.lock`
checksums, the toolchain, the platform, the selected environment, the
contract, the test arguments, the features, both timeouts, the mjutest and
rust-mutants versions, and a fuzz target's corpus. Diagnostics — tracing,
kept temporaries — and parallelism are outside every key.

A run measures up to `[execution] jobs` targets at once in the baseline and up
to that many mutations at once after it — the processors the machine offers,
capped at four, when the configuration does not say, and one whenever a
resource only one test may hold at a time is configured. The two are measured
the same way on purpose: a mutation's budget is derived from what the baseline
measured of the same target, and a duration taken alone is not the one a
target running beside three others will take. Measuring
two mutations at once is not a budget: every mutation still runs, against every
test its route named, and nothing is sampled or skipped. Workers commit
nothing; the answers are put back in the order the catalog has them, so what a
report says is the same however the processors were shared out. Each execution
is given a temporary directory of its own, so two of them cannot meet in one
another's files.

A package whose sources use a directory-reading API — `std::fs::read_dir`,
`walkdir`, `glob`, `globset`, `ignore`, `include_dir!`, or the working
directory — keys the whole snapshot for every target it links and for its
suite. Rust offers no execution observation of file reads that is portable and
unprivileged, so the selection is static and widens rather than trusts. A
package with a build script whose `rerun-if-changed` cannot be read keys the
whole tree as well. Nothing is excluded from testing or reuse by name.

Two kinds of kill are neither recorded nor believed: a kill fuzzing found, and
a kill by a batch or a package suite that does not name the killer. Reuse is
confined to a first round, the whole project, no configured resources, and no
replay. Nothing expires a record; a stale record is removed by being
contradicted.

A reused verdict is one of the executed dispositions: `reused_killed +
reused_survived <= executed`, each carries the `provenance` of the run that
observed it, and its route in the trace records the reuse with no execution
beside it. A reused verdict raises its finding again through the acceptances
of the run reading it.

## Acceptances

An acceptance is human authorization, not a mutation result. It requires a
finding ID, non-empty reason, future RFC3339 expiry, and may carry owner and
ticket. Every mutation marked `accepted` must reference a matching record.
Expired acceptances are ignored.

## DEFECT, INSUFFICIENT, and ERROR

`DEFECT` means user code violated a baseline, soundness, build, or test
contract. `INSUFFICIENT` means execution completed but a survivor, flaky or
inconclusive outcome, unpersisted fuzz kill, excluded boundary, unsupported
Miri operation, or other evidence gap remains. `ERROR` covers incomplete
accounting and toolchain, provider, filesystem, protocol, or workspace
failures.

A limitation is always structured with a stable code. Excludes, estimates,
unavailable metadata, and skipped later phases must never be hidden behind an
assured-looking percentage.
