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
classified as one target per library, and mutations are routed to them: a
mutation only a documented example can notice is noticed by it rather than
reported as surviving. What such a target carries is not coverage but the
files its library is made of, so it reaches every mutation in them and narrows
none of them, which `doctests-routed-by-file` says.

One target per library rather than one per example is not a simplification.
rustdoc merges a file's examples into one compilation, and asking that harness
for one of them by name runs every example in the file — a filter that matches
nothing filters everything out, and a filter that matches one example runs all
of them. So a kill a documented example finds is attributed to the library's
documentation, and `--test-runtool` is what would attribute it to the example.

A library that documents no example has nothing to run and is not a target: a
target that ran nothing would raise a finding about documentation nobody
wrote. A test binary that brings its own harness cannot be asked for one of
its tests and is measured whole, which `custom-harness` says.

A mutation is measured against a suite that passes. A run whose baseline saw a
target fail reports that and measures no mutation: there is nothing for a
mutation to change about a test that was going to fail anyway. A future performance
contract must be explicit rather than treating ordinary benchmarks as tests.

## Mutation routing

A target is a test binary: a library's own tests, one integration test, a
binary's own tests, an example, a procedural macro crate's own tests, or a
library's documented examples. A route names the targets that could notice a
mutation and, for each of them, which of its tests the mutation is put to. The
tests belong to the target rather than sitting beside it, so a route cannot
name tests of a target it does not keep, and cannot keep a target whose tests
it forgot to say anything about.

Reach is decided by the guards the instrumented build compiled in, and the one
run of every target that establishes the baseline is the run that records it.
Each guard appends the site it evaluated under the name of the thread it
evaluated it on, libtest names a test's thread after the test, and what comes
back is therefore per test rather than per target. Nothing is measured twice
because nothing is measured a second time at all: the measurement is the
baseline.

The decision widens whenever the evidence cannot carry it, and every widening
runs more rather than less. The route names which one it was:

| Fallback | What could not be established |
| --- | --- |
| `not-measured` | nothing was recorded at all, so every test of every target runs |
| `position-unknown` | the catalog could not say where the mutation is |
| `outside-blocks` | no instrumented region contains the position |
| `coverage-incomplete` | a target the measurement could have named carries no profile |
| `touch-incomplete` | a target's guards recorded nothing this run can route by |

A target the record does not name is one nothing was established about, and it
stays in the route with every test of it. A site recorded on a thread nothing
names a test after reaches every test of its target, for the same reason.
Being absent from a measurement is not the same as being measured and reaching
nothing, and reading the first as the second turns a kill into a survivor.

Saying that a position reaches nothing is a claim about the code rather than a
gap in the measurement, and it holds only for a target that was measured, was
asked, and answered that nothing of it reached the site. A mutation no such
target reached is `unreached`: nothing runs, and the finding says no test
executes this code. The route names every target that was in a position to
notice and did not. Naming them rather than counting them is what lets
`xtask proofaudit` re-derive the layer rather than confirm that the engine
said it.

A library's documented examples are not among the targets a measurement names.
rustdoc compiles them while cargo runs them, so no instrumented build reaches
them — they are not unmeasured targets, they are targets this measurement is
not about — and they reach a mutation when it is in a file their library is
made of. `doctests-routed-by-file` says so, on every report where one ran.

`unreached` is not `surviving`; both are `survived` in the mutant inventory,
so the accounting equations below are unaffected by how a mutant was routed.

The rule itself lives in the engine, and this runner asks it rather than
keeping one of its own. Two rules for one question disagree eventually, and
the disagreement this pair would make is a kill reported as a survivor.

### Discharging a test a branch proof rules out

A reaching set the measurement decided is narrowed once more where the mutation
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
route the measurement decided with no fallback, and never on one that widened.
It requires that the instrumenter wrote the body's marker at all, because
otherwise no target's silence about the body means anything. A fuzz target is
never discharged: it explores inputs beyond the corpus its measurement was
taken on.

A mutant every reaching target was discharged for is resolved without a single
execution, and reported as a `surviving-mutant`. That no test takes the branch
the mutation narrows is the finding — a real gap in the suite, stated for the
cost of reading a record the baseline already wrote.

### Discharging a test that never saw the mutation make a difference

A reaching set the measurement decided is narrowed a second time by what the
guards recorded. Write reaching for the routing decision as a whole:

```text
reaching(m, t) = touched(m, t)
               ∧ ¬branch-discharged(m, t)
               ∧ ¬(compared(m) ∧ measured(t) ∧ m ∉ infected(t))
```

The guard the instrumented tree carries for a mutation has two sides: the one
the mutant takes when it is active, and the one the original takes when it is
not. For a mutation the engine has a question for, the second side compares the
value the original computed at the mutated site against the one the mutant
would have put there, and records the site when the two differ. Nothing is
built or run for this. The run that establishes the baseline is the run that
records it, and it is the same run that records which test reached which site:
one execution answers both questions, because the questions are about the same
moment.

The engine runs a test binary whole, so what it records is that some test of a
binary saw the site differ and not which one; a binary that never saw it differ
is one whose every test ran the original program and the mutated one through
identical states, and none of them can have observed the mutation. Such a
target is *discharged* with reason `never-infected`. The coarseness costs
discharges and never soundness: one test of a binary seeing the site differ
keeps every test of that binary.

That the recording is a proof is the engine's obligation. A mutant does not
evaluate the operand it replaced, so a question is attached only where leaving
that operand unevaluated changes nothing observable — every operand of the
statement is effect-free — where the recorded comparison is always reached —
the replaced operand cannot panic — and where equal values mean equal
behaviour. That last one is the narrowest, and it is the compiler that enforces
it: the comparison is written against a sealed trait, and a type outside it
does not compile. What is inside is the integers, `bool`, `char`, the unit,
`str`, `String`, `Option<T>`, `Vec<T>`, and a reference to any of them — the
types whose equality is the whole of what a program can tell apart. A float is
refused because `-0.0 == 0.0` holds and `-0.0` is not what the default writes;
an equality that answers about one field while a test reads another is refused
because it would otherwise say a test saw nothing while it watched the
difference, and the discharge would remove the test that finds the defect.

The question is put to the compiler before it is written into the tree that has
to build, on the pass that already asks whether each mutation is accepted. A
site the compiler will not vouch for loses its question and keeps its mutation:
the mutant is executed and measured like any other. mjutest states nothing
about a site the engine did not claim, and holds what the engine does claim to
the recorded kills of every dogfood run through the offline `proofaudit`
infection layer.

The narrowing applies only where the measurement carries it, and everything
else is kept. A mutation the instrumented tree carries no comparison for —
`compared` is false — is absent from every measurement there will ever be, so
its absence from one says nothing. A target the guards did not record carries
no facts at all. Both proofs may answer for targets of the same route; they are
applied in order — branch first, then infection.

The infection layer narrows and never widens. It records per binary, so what it
holds is that some test of a binary saw the mutation make a difference and not
which one: putting a target back on the strength of that would put every test
of the binary back, whatever the record of each said. The case it would be for
— a measurement silent about work the tests do — is the one a fallback already
answers, and a fallback runs every test of every target, which is more than
the widening would.

All three narrowings are proof layers in the sense of
[ADR 0004](adr/0004-proof-layers-not-budgets.md): an execution is removed only
where evidence the run already holds proves it could not observe the mutant,
never by a time budget, a sample, or an exclusion of slow targets, and a layer
that cannot establish its premise keeps the execution.

### Proving that nothing could have noticed

A mutation nothing noticed is a gap in the tests unless there was nothing to
notice. `[mutation] equivalence` asks the compiler about every one of them:
the tree is built, built again with the mutation spliced in, and the
executables the two builds produced are compared byte for byte. Identical
executables are the same programs, and the same program makes the same
observations, so no test can tell the two apart. The argument needs neither a
deterministic compiler nor a correct one
([ADR 0013](adr/0013-codegen-identity-is-the-equivalence-proof.md)).

Identical is what the engine says. `equivalent` is what this run says, and
only where five premises hold: a control still builds the original to the
bytes it built to, no test wrote into the tree while it was being measured,
the package holds no `unsafe`, the route was decided by the measurement with
no fallback and named at least one target, and no killer was recorded. The fourth carries the layer.
A mutation of a function no test calls is dropped by the linker and the
artifacts come out identical for the opposite of a reassuring reason, so a
mutation nothing reached keeps its finding whatever the compiler did with it.

The comparison is made under the project's own test profile, because that is
the profile the tests run under: `x + 0` and `x - 0` are the same instructions
at `opt-level = 1` and different ones at the `opt-level = 0` cargo gives a
test profile by default. On a project that leaves it at the default the layer
proves almost nothing, and says so rather than appearing to have looked.

This removes findings and never executions. Every test that reaches the
mutation has already run by the time the layer does, and turning the layer off
leaves the finding in place.

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
cataloged      = rejected + executed + unreached + equivalent
executed      >= killed + survived + timed_out
accepted      <= survived + unreached + equivalent
reused_killed <= killed        reused_survived <= survived
```

`equivalent` is a column of its own rather than a part of `survived`, because
a reader who cannot tell "nobody noticed this" from "nobody could have" cannot
act on either.

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
nothing was ever run against. Fuzz targets disqualify a survival in both
directions.

#### A mutant the evidence cannot say nothing reaches

Settled by running the targets the route widened to, so the claim is recorded
as the conjunction of those targets' own behaviour keys: a target that enters
or leaves the route refuses reuse where one key over the package would have
hidden it. A record this run cannot resolve to the targets its own baseline
saw pass is neither believed nor written, because half of a set is a smaller
claim wearing the same name. A mutant the premise of `unreached` holds for is
a claim about the code and is reused by nothing.

#### A timeout

A timeout is not a proof about the mutant. Reusing one keeps a finding and
never removes one. Each mutation command, and the probe command that measures
the same target, gets five times its measured baseline duration plus five
seconds, with a 30-second floor; the contract caps calibration at 30 minutes
for `standard-v1` and five hours for `deep-v1`, and `[execution].timeout` is a
further upper bound.

An expired budget buys one measurement with the machine to itself. The budget
is derived from a duration the baseline measured, and a duration measured
while other test processes were running is a fact about the load as much as
about the mutation, so before a run decides that time really ran out it stops
starting anything else and measures once more. What that measurement observes
is what stands: a mutation that completes under it was observed completing,
and only a budget that expires again with nothing else running is a timeout.
This is not a retry policy — one expired budget buys exactly one quiet
measurement, and the recording says of every execution whether the machine was
given to it.

An expired budget remains inconclusive under every bound. The record names the
target time ran out under as the **last** of its executed targets, stored in
execution order. `mjutest replay <finding-id>` bypasses
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
a kill by a batch that does not name the killer. Reuse is
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

## Parts of one catalog

`mjutest verify --shard K/N` divides the judging. Every part measures the whole
baseline, because a mutation cannot be judged against tests that were not run;
what a shard divides is which mutations are put to those tests. The rule is the
engine's, so both products cut a catalog the same way: the dense catalog index
modulo N, counting K from one. Two runs of the same tree therefore agree about
which part holds which mutation without saying a word to each other, and every
mutation belongs to exactly one part — nothing is sampled, nothing is skipped,
and no execution is paid for twice.

That is what keeps dividing the work out of
[ADR 0004](adr/0004-proof-layers-not-budgets.md)'s way. A budget decides not to
run something; this decides which machine runs it. The parts balance by count
rather than by cost, so a part holding a slow mutation takes longer, and how
long is something to measure rather than to predict.

A part concludes `PARTIAL` and records its shard. It assures nothing on its own:
the mutations it did not judge are not mutations nothing noticed, they are
mutations nobody put to a test, and a report that called that an assurance would
be claiming the one thing it did not look at. A finding in a part is a finding,
so a part that found a defect says `DEFECT`.

`mjutest merge <REPORT>…` writes the report the whole would have written. It
refuses five things: no parts at all, parts that disagree about the tree, parts
that disagree about the configuration, parts that disagree about the contract,
and two parts that both judged one mutation — the last says they were cut with
different values of N. The last three are this runner's and not the engine's. A
run report is a collection of what running something said, and two collections
add up whatever produced them; an assurance report is one claim, that a contract
was met, and a claim assembled from a part that met it and a part that met
something else is true of neither.

The mutant rows of the whole are the union of the parts'. Its accounting is
derived from that union rather than added up from what each part counted, so the
whole's columns say what the whole's own records say. The exception is
`accepted`, which is a fact about a reviewer rather than about a mutation and
appears in no record, and is therefore summed — each mutation belongs to one
part, so each acceptance is counted once. No score crosses a merge at all: two
ratios over different denominators average into a number no run observed.

Every run's identity carries its shard, so a part never reads back the whole's
stored answer and a whole never reads back a part's.

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
