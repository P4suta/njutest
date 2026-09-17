<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0023 — A run may not conclude from how it measured

## Status

Accepted, 2026-09-18. Bounds every finding this workspace can raise.

## Context

Seven times now, in two independent lines of work, the same defect has been
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

## Decision

**A run concludes about its subject. It may not conclude from an artifact of
how it measured.** Before a finding is raised, the premise it rests on has to
be something the run actually established, not something the measurement's own
shape makes true.

Four ways out, in the order to try them:

1. **Capture what is missing.** The gap is usually in the measurement rather
   than the reasoning: record whether the exchange came past, record the status
   line beside the parsed code. Then the premise is checkable rather than
   assumed.
2. **Count only what was answered.** An attempt that established nothing is
   not a chance the subject failed to take, and the count is the whole weight
   of a claim like "put to 31 and noticed none".
3. **Refuse at a scope that cannot answer.** A part says `SCOPE_ASSURED` and
   not `ASSURED`; the same rule one level down means a part does not raise a
   finding about a target over the whole catalog, and `njutest merge` raises
   it from the combined records.
4. **Abandon the inference.** Weakening an assertion and the first-killer
   artifact have no repair, because there is no measurement that would make
   them sound. A framing that cannot be made honest is retracted rather than
   qualified.

## Consequences

**A finding is not declared before it is demonstrated.**
`every_finding_kind_a_report_can_carry_is_one_a_test_names` refuses a
`FindingKind` that no test produces. That gate is what caught the first of
these: the premise fell apart in the first three lines of writing the fixture
that was meant to exercise it. A gate that makes somebody demonstrate a
finding is worth more than one that checks they spelled it consistently.

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

**Nothing here is a budget.** No threshold, no cutoff, no "more than N is
suspicious" ([ADR 0004](0004-proof-layers-not-budgets.md)). A finding either
rests on what the run established or it is not raised.
