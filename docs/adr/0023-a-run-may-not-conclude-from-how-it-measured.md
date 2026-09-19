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

**Closing a set makes a match total, not honest.** Both halves of this have
now been shipped by somebody applying the rule correctly. `telling::spot` was
converted to an exhaustive match over a closed `Outcome` and mapped
`Unconfirmed` and `Errored` onto the arm that says *the tests ran this line
and passed anyway* — a sentence about a mutation nothing could be measured
for. No arm was missing. The rule catches "somebody forgot a case"; it does
not catch "somebody wrote the wrong one", and the second is the more expensive
because the match looks finished.

What catches the second is matching over the right type. Reading through
`Blind` rather than `Outcome` means the outcomes that are not holes have no
spot to be, so there is no arm to give them the wrong sentence; binding a
payload per variant — `Killed { by }`, `TimedOut { on }` — means the name and
the sentence about it cannot come from two places and disagree. **A type is
chosen so that the wrong arm has nothing to be written about, not so that
every arm is written.**

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

**A catalogue is derived from a run of the same shape as the runs that will
answer it.** A seam catalogue was assembled from a recording read after the
mutation phase, which runs the suite once per mutation — so a seam that saw
one exchange had a catalogue of twenty, nineteen of them copies, and the
report stated a hundred and thirty-three questions the run had established
nothing about. Every one a hole the run invented by measuring.

Reading the recording earlier does not fix it: the baseline builds and
verifies before it measures, so it runs the suite more than once too, and the
repeats are the same exchanges counted again. A fault names an exchange by its
place in the order and is put by running the suite *once*, so a catalogue
assembled from two runs holds questions that cannot be put by construction —
and states each of them as a question nobody put.

This is the apparatus class again, and it is the member of it worth stating
separately: **it is provable rather than only assertable.** That a catalogue
derived from more than one run holds questions a single run cannot answer is
visible without running anything, which means it can be closed by
construction rather than only guarded by a test. It is closed twice here: the
phase takes a value that can only be made around a run, and recording stops
when that run ends.

**Some of it is a claim about the apparatus, and only a test reaches that.**
Two defects in the seam layer were of a kind none of the ways out above
touches. A fault run on one seam drove traffic through another, and that
traffic became the second seam's baseline — a catalogue derived from a
program already being perturbed, which is the contamination this whole
product exists to find in other people's suites, happening inside ours. And a
test helper took a connection count as an argument, so a number that was
wrong about the world outside the program hung for sixty seconds instead of
failing.

Neither is a type error: every `usize` is a valid count, and no signature
distinguishes a clean baseline from a contaminated one. Neither is a
catch-all, a wrong arm, or a sentence a stranger would catch by reading the
output. What they have in common is that the claim is about the **measuring
apparatus** rather than about the subject — and the only thing that reaches
that is somebody who understands the measurement writing down what must
remain true of it, as a test whose subject is the apparatus.

Both were caught that way, and the order is the part to keep: the test naming
the contamination was written after the defect existed and before it was
fixed, so it says what must be true rather than what the code does.

**And some of it needs an execution environment the author does not have.** A
caller's request returns when the interposer closes the connection to it, and
what the run reads afterwards is written down after that — so a question asked
the instant the last caller was answered gets an answer from a state the run
has already left, and reports a fault that was put half a microsecond ago as a
question nobody asked. Fifteen clean local runs said nothing; CI found it
twice, once in the coverage job and once on all three platforms at the same
time, running the identical command somewhere slower.

What made the fix trustworthy was not that it looked correct. Twenty
milliseconds inserted between the carry and the store turned the exact test CI
had named red; the fix turned it green; removing the fix with the window still
wide turned it red again. Accepting a fix because it was obviously correct is
the same mistake as sizing a hand-computed count correctly instead of removing
it.

So the class has three members with three different reaches, and knowing which
one is in front of you is most of the work:

| | Reached by |
| --- | --- |
| a catalogue derived from the wrong shape of run | proof, without running anything |
| one measurement contaminating another | somebody writing down what must stay true |
| a question answered from a state the run has left | **running it somewhere you are not** |

**A test that names what it is about survives a bad merge.** The mechanism is
that the name is a claim, so the test fails when the claim stops holding —
whoever stopped it holding and however.  Two branches
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

**A zero value that means "no answer" is the shape this keeps taking.** Five
layers, one remedy. `Blind` had `Undecided` among the ways a build is a hole,
so a build that established nothing read as one the tests were blind in.
`RouteRecord` derived `Default` while `Granularity` has none — correctly,
because there is no granularity a route is decided at when nobody decided it
— so a routing that did not happen read as one that did. `Why` would have
collapsed *no recording* into *nothing stands behind this*, in the command
whose entire job is explaining why. And a review loop that offered `Accept`
over an unsettled place would let somebody decide a measurement that never
happened may stand.

`Provenance` was the fifth: `cached: false` beside `source_run_id: None` is a
zero value meaning *nobody said*, and `Established::Here` is the same move.

Every one of them was fixed the same way: **make the absence a case rather
than a value.** The reason it keeps working is exact rather than stylistic: a
zero value is indistinguishable from a real answer that happens to be zero,
and a case is not. A default, a sentinel, a zero and an empty list are all a
program saying *nothing* in the grammar it uses for *something*, and every
reader downstream has to remember which one they are holding. A variant does
the remembering.

The tell is a type whose `Default` is reachable in a path where the thing it
defaults to was never asked. If you cannot name what the default means
without the word "not", it is a case.

**And `#[derive(Default)]` on a struct is where the compiler stops helping.**
A derived default is a claim about every field, checked field by field, with
no view of whether the whole means anything: `RouteRecord` derived one while
`Granularity` has none — correctly, since there is no granularity a route is
decided at when nobody decided it — and the result was a routing that did not
happen, synthesised out of fields that were each individually fine. That is
the paired-field defect at the level of a whole record, and the only defence
is not deriving it.

**A workaround looks like design.** `#[non_exhaustive]` on a type whose
callers render it forces a `_` arm outside the crate. One author, having put
the attribute there, later wrote an accessor whose only job was to answer the
question a `match` would have answered — sparing a reader the arm the
attribute forced. Nothing about the result looks wrong: it is a small method
with a reasonable name.

That is the shape worth noticing, because it is the one case where the defect
leaves no wound. A missing arm is visible; a method that exists only because a
type would not let somebody match is a design decision to every later reader.
The tell is a projection of `self` that the domain would never have asked for,
and it is offered here as a thing to notice rather than a thing to gate: a
rule broad enough to catch it would catch half of any presentation layer.

**A ledger that names line numbers asks the question at the right moment, and
that is worth the friction it looks like.** A shrink-only waiver file keyed by
line — `xtask/seam_allowlist.txt`, `wildcard_allowlist.txt` — makes every
refactor that moves a line ask *is this waiver still needed?* of somebody who
is already looking at the code. Two waivers went away rather than being
renumbered the first time this happened, because the answer turned out to be
no and nobody would have gone to ask otherwise.

So far, every time the ledger has asked, the answer has been no: three waivers
across two branches, all three deleted rather than renumbered. Two occasions
is not proof, and it is better evidence than anybody expected this early.

It was not designed in; the renumbering was expected to be pure friction. It
is written down here because the obvious improvement — match the waiver on
content so it survives a move — would delete the property, and somebody
proposing that should have to argue with this paragraph first.

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

**Choose the key that survives what the reader just did.** `watch` compares
two rounds by naming each gap and asking which names went away. The obvious
name is the locator a reader types — `path:item:rule@line` — and it is the
wrong one, because this surface exists for the seconds after somebody saved
the file. Insert a line above a gap and the line-keyed name changes, so one
round reports a gap closing and another opening at the same spot, and the
person reads that as having fixed something and broken something else in one
keystroke. The run learned nothing about either; the diff concluded from how
it addressed things. Keyed on the locator without its line, two gaps of one
rule on one item collapse into one entry, and that is the right trade: a
watcher is reading for change, and the second of two identical gaps was never
news.

It is the apparatus class from the other end. There, a run concluded from how
it measured; here, a run identifies something by an accident of how it
measured. Both have the same tell: the wrong key is the one that is already
lying there — the line number, the ordinal, the position in a list — and the
honest one costs something, because you have to decide what identity means
before you can compute it.

The same sentence applies to a proof that stopped because the caller's clock
ran out. *The bound was too small* and *this argument cannot be made symbolic*
are facts about the code, and a reader acts on both. *We stopped waiting* is a
fact about the apparatus, and rendering it as a third bullet of the same list
invites the conclusion that the code resisted proof. It did not; nobody
asked it for long enough. Two types rather than three arms, so no renderer can
offer a knob that would not have helped.

**A gate that cannot see a shape reports that the shape is not there.**
`wildcard-over-our-own` read a match's arms to learn which set was being
matched. syn 3 keeps a guard inside the pattern rather than beside the arm, so
`Payload::Route { route } if route.mutant == id` arrives as a `Pat::Guard` and
the reader of `Pat::Path | Pat::TupleStruct | Pat::Struct` returned nothing for
it. Every match whose variant-naming arms all carried guards was therefore
invisible: not exempted, not waived, not seen. The gate passed, and passing was
its way of saying *I found no such arm here*, which was true about the gate and
false about the code. Seeing them turned up two arms in `validate.rs` that no
run had ever reported.

Three plausible mechanisms were reasoned out and all three were wrong; the
answer was a dependency's API change, which is not a thing staring at our own
code can show. The minimal reproduction — the same source with and without the
guard, one line of output each — settled it in a minute.

**Correct by accident and correct are different states for a gate to be in,
and only one of them survives a refactor.** Exempting those matches turns out
to be right: with a guard on every arm that names a variant, nothing is covered
unconditionally, so the compiler demands the rest and no reviewer could have
refused it. But that was not what the code was doing, and a behaviour nobody
chose is one the next person removes without knowing they changed anything. It
is now a named function with a stated direction of error: one unguarded naming
arm and the exemption stops, which costs a ledger line rather than a blind
spot. And the two readings of a guard deliberately disagree — the walk looks
*through* a guard to learn what is being matched, and refuses to look through
one to decide what catches everything left, because `_ if ready()` catches
nothing on its own.

**A waiver against something the compiler demands is a decision nobody made.**
Of the ledger's forty-four lines, thirty-eight named arms that could not have
been left out: an enum of ours saying `#[non_exhaustive]`, read from another
crate, and an integration test is its own crate. A reviewer reading the file
had been told those were somebody's choice. The ledger is three lines now, the
count is in the pass line, and a file of three is one somebody opens.

**A capability with a test is a capability somebody believed shipped.**
`Interposer::during()` had a test, passed it, and production never called it.
The test measured the function correctly; the absence of a caller made a
correct measurement say nothing about the product. This is the apparatus class
one layer further out — not a run concluding from how it measured, but a suite
concluding about a product from a measurement of a part nothing uses. The
milestone that named the gap named it in the right place, and the capability
filling it was already there.

A gate for it does not exist, and the reason is worth more than the gate would
be. Of 718 public functions in production source, 125 are named only from
tests — and most of those are not findings, because `rust-mutants` is a library
whose callers are not in this tree. The predicate that separates them is *a
public function in a crate whose public surface is incidental*, which is a fact
about the crate and is written down nowhere.

**A gate cannot ask a question whose premise nobody wrote down.** Twice in one
day: *could this arm have been left out* needed to know which of our enums say
they may grow and which crate reads them, and *is this crate's public surface
an API* needs somebody to say so per crate. Neither is computable from the
source, both are declarable, and in both cases the first instinct was a
cleverer walk. A declaration in `Cargo.toml` has the property the ledger has —
the cost lands on whoever is creating the crate, at the moment the decision is
being made, rather than on a reader much later who has to reconstruct it.

**A file that is a projection of the tree has no meaningful textual merge.**
Every conflict in the catch-all ledger today was resolved by regenerating it
rather than by merging two lists, because two projections of two different
trees are not two versions of one document. `git` cannot know that. Where a
ledger is derivable, the command that regenerates it is the conflict
resolution, and hand-merging it is a way of writing down an answer no tree
supports.

**A derive propagates an obligation the type cannot honestly meet.** The
record of one process execution derived `Default`, for one test builder's
convenience. Every field of a `Default` struct needs a default, so the closed
set saying how a process ended needed one too — and there is no such value,
because a process that has not ended has not ended. The language does not say
so. It takes `#[default]` on whichever arm is first and moves on. What came
out was a published example, in the file somebody opens to learn the recording
format, saying that a process which ran `cargo metadata` and exited zero could
not be started or supervised at all.

Nobody wrote that. Declaration order wrote it, which is why it was invisible
to review: there is no line to disagree with.

The rule this gives is better than the one it replaces, because it says when to
look rather than what to look for. Before adding a derive, ask of each field:
*does this type have a value that means nothing was decided?* Where the answer
is no, the derive is asking the type to invent one. And note which way the
obligation ran — it started at a test builder, travelled through a struct, and
landed in the domain type written to make that exact claim unrepresentable.
The convenience was two layers from the lie.

**A refusal that something downstream is free to resolve is not a refusal.**
Retiring an outcome name so that `parse` returns nothing for it is the whole
of a deliberate break: a word that meant two things must stop a reader rather
than resolve to whichever of them they guess. A test helper wrote
`parse(name).unwrap_or(Outcome::Errored)`, and the break became an error
count one layer down. The refusal was correct and the caller was permitted to
throw it away.

It is the same shape as the paragraph above it, at a different depth: both are
the language offering to fill a hole somebody left open on purpose. `None` and
`#[default]` are the two offers, and taking either one silently is how a
decision stops being one.

**Nothing here is a budget.** No threshold, no cutoff, no "more than N is
suspicious" ([ADR 0004](0004-proof-layers-not-budgets.md)). A finding either
rests on what the run established or it is not raised.
