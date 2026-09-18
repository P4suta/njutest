<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0021 — A claim is a perturbation, an observer, and a decision

## Status

Accepted, 2026-09-18 (user decision). Implemented for the mutation phase by
`report::Decision`, `report::ObserverAccounting`, and the
`DecisionsDoNotAddUp` invariant of `report::audit`. Extends
[ADR 0004](0004-proof-layers-not-budgets.md) rather than replacing it, and
governs the shape of every evidence source added after it.

## Context

Mutation testing perturbs one thing — the source syntax of one process — and
asks one party whether it noticed — the test suite. Both are accidents of
where the technique was born rather than anything essential to it, and this
repository has been quietly working around them for a while.

The clearest symptom is `compile-rejected`.
[ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md)
decides that acceptance is the compiler's: a candidate the build refuses is
condemned with its diagnostic and reported as `compile-rejected` rather than
as a survivor. That is right, and the count has been in the report since M2.
What the report never said is what the count *means*. Byte splicing preserves
syntactic well-formedness, so a refusal is the compiler's static semantics
turning down a program — the type system catching a fault. Recorded only as
work the run did not do, it reads as noise. It is the one measurement in the
toolchain that nothing else makes, and the report was discarding it.

The second symptom is in the roadmap's own conclusion about propagation. It
observes that a model checker "says 'no input distinguishes these two', which
is a stronger answer than any test run", and then sets it aside because it
"does not reach a **killer** layer". The reason a proof layer must fail toward
running more is that it *removes* an execution. A model checker *adds* one, so
it is not standing behind that wall at all. The obstacle was the vocabulary:
there was a word for a party that removes work and no word for a party that
notices.

The third is that the counts a report already carries overlap. `executed`
holds `killed` and `survived` both; `reused_killed` is part of `killed`. They
answer how much of each kind of work a run did, which is a real question, but
no arrangement of them answers *what stands behind the verdict*.

## Decision

1. **The unit of assurance is a claim, and a claim is three things**: a
   perturbation, an observer, and how the pairing was decided. Every evidence
   source added from here names its values for the three, and nothing is
   integrated as "another kind of test".
2. **The observers are a closed set the compiler checks.** For the mutation
   phase today they are the type system and the tests. A new observer is a new
   variant, and the exhaustive matches that break are the decisions it forces.
3. **`Decision` is a partition, and the report is held to it.** The six ways a
   mutation can be decided — `types`, `tests`, `proved`, `unnoticed`,
   `unreached`, `undecided` — cover the catalog exactly once.
   `report::audit::validate_for_persistence` refuses a report where they do not
   add up to `cataloged`, so every path that builds an accounting is held to it
   rather than each one being chased.
4. **`Undecided` is a first-class answer.** A run that could not decide a claim
   says so, in a column of its own, and names why. It is a gap in the
   verification rather than in the project. Every other tool in this space
   spells the same state as an absence — a sample not taken, a budget expired,
   an exclusion — and an absence is indistinguishable from a decision nobody
   needed. This is [ADR 0004](0004-proof-layers-not-budgets.md) decision 1 said
   in the vocabulary of the whole rather than of mutants.
5. **The existing counts keep their meaning.** `rejected` stays where it is in
   `cataloged = rejected + executed + unreached + equivalent`, and no score
   changes denominator. The observers are a second reading of the same
   dispositions, not a replacement for the first.
6. **One table maps outcomes to decisions**, `Decision::OUTCOMES`, and a ledger
   test holds `docs/report-v1.md` to it in both directions. An outcome the page
   forgets is one whose standing a reader cannot tell; one it lists twice is one
   they would count twice.

## Consequences

- The type system becomes visible as an observer, which is a measurement a
  reader can act on: how much of what could go wrong is being caught before any
  test runs. It costs an accounting field and no run time, because the
  measurement was already being made.
- A model checker, a property, a runtime invariant and a wire-level fault each
  arrive as values of the three axes rather than as a new subsystem with a
  vocabulary of its own. [ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md)
  already requires a new method to arrive as a sibling under `assure/` with its
  own contract clause, trace vocabulary and audit layer; this says what the
  sibling has to be *about*.
- Adding an observer or a way to decide is a change to the partition, so it
  cannot be done quietly: the invariant fails until every path that counts has
  been taught the new column, and the exhaustive match on `Violation` fails
  until somebody has written the sentence a reader sees.
- A verdict now has a shape a reader can check without trusting the tool: the
  columns add up to the catalog, and each column names who stands behind it.
