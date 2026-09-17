<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0023 — A run may not conclude from how it measured

## Status

Accepted, 2026-09-18. Bounds every finding this workspace can raise.

## Context

Eight times now, in two independent lines of work, the same defect has been
written and then caught. Each time it looked like a new inference about the
suite. Each time it was a restatement of how the run had measured, wearing
the grammar of a conclusion.

**A test whose assertion can be weakened without failing asserts nothing.**
Weakening always survives, by construction: `assert!(total > 3)` with `>`
changed to `>=` passes whenever the original passed. "Survived, therefore
nothing rests on this assertion" is not an inference from the measurement; it
is a definition of weakening. Strengthening is the mirror and always kills.
Neither direction says whether the test constrains the code.

**A target that killed nothing constrains nothing.** The mutation phase stops
at the first target that notices
(`assure/mutation.rs`, the loop over `route.reaching()`), so `killed_by` names
the first killer in route order and not a distinguished one. A target that
killed nothing may be one that is always later in the order than another that
also would have killed. The conclusion restates the optimisation.

**A seam question nobody was asked is a question nobody noticed.** If the
suite takes a different path under a fault, it never reaches exchange N, every
test passes, and a run with no record of whether the exchange came past cannot
tell that apart from nothing noticing.

**A target that errored was a target that did not notice.** Counting an
`unconfirmed` or `errored` answer into "was put to N mutations and noticed
none" weighs an accusation with attempts that established nothing. It did not
fail to notice; it was never asked a question it could answer.

**A part may answer for the whole.** A shard judges a slice, so a target
silent in part one and killing in part two is accused by part one — and a
merge that concatenates findings carries the accusation through
unrecomputed.

**A build that established nothing is a build the tests are blind in.** A
mutation `survived` in one build and `errored` in another, rendered as
"ran, not noticed in debug, release", is false about release in the direction
that costs somebody work: they go looking for a missing assertion and the
harness is broken.

**A fault that would produce the answer already there is not worth asking.**
True, and unknowable from a recording that kept the parsed status code and
discarded the status line, because the injection rebuilds a reason phrase and
the recording no longer says what the original one was.

**A mutation that ran out of time was decided by the tests.** The engine is
right to say so — the process hung with the mutation active, and that is a
detection. njutest is measuring something else: it gives a timeout its own
column, and raises a finding reading *an expired budget establishes nothing
about the mutation*, because a bound is a budget and a result resting on one
is not a proof ([ADR 0004](0004-proof-layers-not-budgets.md)). A third table
then said a timeout was a detection at the second-highest standing, so a
build that timed out was not a hole and a release-only timeout vanished from
`blind_in` entirely.

That last one is a different sub-kind and the reason it is here: nothing about
it was *wrong to write*. Three tables each said something defensible about the
same fact, and the defect was that there were three. A value with more than one
source of truth disagrees eventually, and the disagreement is invisible until
somebody reads all three at once — which is not a thing to arrange. One table,
read through by everything that needs it, cannot contradict itself.

## Decision

**A run concludes about its subject. It may not conclude from an artifact of
how it measured.** Before a finding is raised, the premise it rests on has to
be something the run actually established, not something the measurement's own
shape makes true.

Five ways out. Which comes first depends on where the fault is: when the
measurement is short, capture what is missing; when the measurement is there
and the type is too wide to say what was measured, make the wrong reading
unrepresentable.

1. **Make the state unrepresentable, and say each fact once.** A `String`
   whose legal values are a fixed list, a `match` with a `_` arm over a closed
   set, one enum whose arms answer to different invariants, and one fact
   spelled out in three tables are all the same thing: a program with room in
   it for a sentence nobody meant. Close the set and the compiler
   refuses the defect rather than a reviewer catching it. Three of the seven
   above are of this kind, and each was found the moment the type narrowed:
   `Decision::blind()` returning `Option<Blind>` over three values rather than
   a `filter` over six turned "which builds is this a hole in" into a question
   the compiler makes somebody answer for every decision there will ever be,
   and splitting `Blindness` in two made `njutest verify` stop counting a
   timed-out mutation as a gap in somebody's tests, because the function that
   counts gaps could no longer be handed one.
2. **Capture what is missing.** The gap is usually in the measurement rather
   than the reasoning: record whether the exchange came past, record the status
   line beside the parsed code. Then the premise is checkable rather than
   assumed.
3. **Count only what was answered.** An attempt that established nothing is
   not a chance the subject failed to take, and the count is the whole weight
   of a claim like "put to 31 and noticed none".
4. **Refuse at a scope that cannot answer.** A part says `SCOPE_ASSURED` and
   not `ASSURED`; the same rule one level down means a part does not raise a
   finding about a target over the whole catalog, and `njutest merge` raises
   it from the combined records.
5. **Abandon the inference.** Weakening an assertion and the first-killer
   artifact have no repair, because there is no measurement that would make
   them sound. A framing that cannot be made honest is retracted rather than
   qualified.

**A catch-all is wrong over a set this workspace closes and right over one it
does not.** The test is whether the values can be listed from this repository's
own source. A mutation's outcome, a build's decision, a fault's rule, a thing
the doctor checks: all written here, all closable, and a `_` over any of them
is this defect waiting. A method arriving over LSP, a line read out of a file
somebody else wrote: not ours, growing without us, and a protocol that says a
server answers an unknown request rather than refusing it. There the catch-all
*is* the handling — with the same condition as everywhere else, that what it
could not read is counted and travels with the answer rather than being
dropped. Both kinds are named in the code as what they are, so the next person
applying this rule does not arrive at the second with a patch.

**A check the type makes impossible is deleted rather than kept.** An audit
variant that refuses a value the enum can no longer hold does not add a
second guarantee; it says the type is not trusted, and it is one more place
to drift out of step with the model it is checking. When a state becomes
unrepresentable, the runtime refusal of it goes with it.

## Consequences

**A finding is not declared before it is demonstrated.**
`every_finding_kind_a_report_can_carry_is_one_a_test_names` refuses a
`FindingKind` that no test produces. That gate is what caught the first of
these: the premise fell apart in the first three lines of writing the fixture
that was meant to exercise it. A gate that makes somebody demonstrate a
finding is worth more than one that checks they spelled it consistently.

**A test that names what it is about survives a bad merge.** Two branches
that both changed `verify.rs` were resolved by taking one side whole, which
dropped the loop that measures every build a project names. Nothing in the
conflict markers said so; a resolution is the one place in this workflow where
correctness is decided by somebody reading, and it is the place no type can
reach. What caught it was
`a_mutation_only_one_of_the_builds_notices_is_a_survivor_that_names_the_other`,
which failed on a report whose `blind_in` named no build at all. A test called
`verify_works` would have gone green on a run that had quietly stopped
answering one of its questions.

**The tests for a finding are mostly about the traps.** Of the four that hold
`hollow::found`, three are: a target never asked is not accused, a target
outranked every time is not accused, a run that reused every answer accuses
nobody. The happy path is the cheap one to write and the one that was never in
doubt.

**An audit that copies the defect is worse than no audit.** `xtask`'s
independent re-implementation of the hollow-target question carried both holes
the implementation did, so agreement between them meant nothing. A second
implementation is evidence only where it was derived from the question rather
than from the first implementation.

**A closed set is matched exhaustively rather than defaulted.** `telling::about`
matches every `FindingKind` with no catch-all, so a new kind does not silently
inherit another's sentence. The same applies to a mutation's outcome and to a
build's decision: a `_` arm in either is this defect waiting to be written
again by somebody who adds a variant.

**Two things that answer to different invariants are two types.** A gap in the
tests has a test that closes it and a `replay` that proves the closing; a gap
in the run has neither, and offering either tells somebody they succeeded at
something they did not do. Held in one enum, that invariant is a note for
whoever adds the next arm. Held apart, `Blindness::replay_proves` exists and
`Unsettled` has no such method, so the briefing's only function that offers a
`replay` is the one that takes a `Blindness` — and the wrong briefing cannot
be written rather than being caught in review.

**Nothing here is a budget.** No threshold, no cutoff, no "more than N is
suspicious" ([ADR 0004](0004-proof-layers-not-budgets.md)). A finding either
rests on what the run established or it is not raised.
