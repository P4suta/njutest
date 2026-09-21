<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0015 — The guard is the infection probe

## Status

Accepted, 2026-09-08.
Implemented by `syntax::branch::Comparable`, the `differing` entry of the runtime of [ADR 0011](0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md), and the `never-infected` layer of `Session::proof_against` (E11).

## Context

The infection question is the second of the three ways a mutant survives: the test reached the mutation, and the mutation computed the same value anyway.
A test that never saw the site answer anything but what it replaces ran a program indistinguishable from the unmutated one, and running it against the mutation establishes what the baseline already established.

Answering it cost a **second instrumented tree**. The probe tree rewrites each site to evaluate the mutation beside the original and append the pair to a log, and it is a build of its own and a run of its own on top of the tree the run will actually use.
On a workspace where preparing is already most of the wall clock that is not a layer a default run can afford, and `--probe` is off by default because of it.

Meanwhile the instrumented tree already holds **both** branches at every site.
A guard is `active(i) && (mutation) || !active(i) && (original)`: the mutation is written there, in the file, one branch away from what it replaces.
What stopped a run from evaluating both is that evaluating the mutation is running the program's code — a call, an index, an arithmetic overflow — and a measurement that changes what the baseline does is not a measurement of it.

But [ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md) already has the machinery that says when it does not.
A branch proof rests on the whole condition of an `if` or a `while` being **inert**: identifiers,
literals, `!`, `&&`, `||`, parentheses, comparisons and casts, with the compiler vouching through a sealed trait that each comparison's operands are primitives and each cast's operand is one.
That question is put in the witness tree, which the one `cargo check` of `prove::establish` already builds.

## Decision

Where the compiler has vouched that a condition is inert, the guard evaluates both of its branches and records every time they parted.

The edit has to sit on an operator token the connectives of the condition reach — `<`, `<=`, `>`, `>=`, `==`, `!=`, `&&`, `||`.
Every rule that fires there swaps the operator for another of the same class, so the mutated condition is the same operands under another operator: inert for exactly the reason the original is, and vouched for by exactly the same witnesses.
The branch proofs of ADR 0014 need the edit to be *decreasing* and need the body it gates; this needs neither, so it covers strictly more edits in the same conditions and asks the compiler nothing the branch proofs did not already ask it.
One condition is witnessed once however many questions rest on it.

The guard becomes `… || !active(i) && differing(i, (original), || (mutation))`, which answers what the original answers and records `i` where the two disagreed.
It is a call rather than a block for the same reason the rest of Form C is one expression: a block would be a temporary scope the site did not have.
A run with nothing to record loads one relaxed atomic and never builds the closure's answer at all.

The record is the same log the reach record uses, on the same run: the one every target already makes with nothing activated to verify the baseline.
So the layer costs **nothing** — no tree, no build, no run — and it is per test,
because libtest names each test's thread after the test.

A target whose record never names the mutant is discharged `never-infected`.
A target that is kept is asked only for the tests that did see the two branches part.

What a guard may compare is decided three times over, and the run trusts the last: the syntax offers it, the compiler vouches for it, and the **instrumenter reports which guards it actually wrote the call into**. A form that cannot hold the call — a value position, a statement — reports none however much it was offered, so no proof rests on a comparison no guard makes.
That set travels to the audit in `touched-v1.json` as `narrowing.compared`,
because an absence in the record is evidence only where something was recording.

The probe tree stays, behind `--probe`, for the return replacements the syntax cannot call inert.

[ADR 0016](0016-the-probe-tree-is-a-tree-nobody-needs.md) took that back: the probe tree answered return replacements and nothing else, and a return replacement writes a constant, so a guard can compare against it without evaluating anything twice.
The tree is gone and the layer covers more than it did.

## Consequences

- `never-infected` is a default layer for the first time.
  It needs no flag, no extra build, and no extra process.
- The same record narrows per test, so a target the layer keeps still runs fewer of its tests than before.
- `narrowing` also carries the marker each branch proof rests on, keeping only the markers the instrumenter wrote.
  That closed a hole ADR 0014 left: a body inside a guard's own site takes no marker, and `branch-never-taken` used to read the resulting silence as "nothing entered the body".
  It now falls back to the coverage region, and with nothing measured the target runs.
- `cargo xtask engine-audit` re-derives both kinds of discharge from the record alone.
  Before this it could only re-derive `branch-never-taken` from a coverage build and could only ask whether a probe log existed at all.
- The layer says nothing about the conditions the compiler refuses — an operand of a type the sealed trait does not cover — and nothing about arithmetic or calls.
  Returns were the probe tree's and are now the guard's too ([ADR 0016](0016-the-probe-tree-is-a-tree-nobody-needs.md)).
- With this in, reach and infection are both answered by one run of the unmutated program.
  Propagation is not, and cannot be: whether a difference reaches an assertion depends on everything between the site and the assertion, which is either a dataflow engine ([ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md) decision 5 keeps one out) or an execution.
  The whole-program case is already answered by [ADR 0013](0013-codegen-identity-is-the-equivalence-proof.md),
  and it costs a build.
  `docs/roadmap.md` says what follows from that.

## Alternatives

- **Evaluate both branches at every site.** Evaluating a mutation is running the program's code, and a measurement that panics, allocates, or overflows where the baseline did not is not a measurement of the baseline.
  The witness pass is what draws the line, and it draws it conservatively.
- **Infer the answer from the reach record.** A test that reached a site says nothing about whether the two branches agreed there; that is the whole distinction between the reach layer and this one.
- **Keep the probe tree and make it cheaper.** It is a second build of the workspace whatever it does inside.
  The cheapest build is the one that is not made.
- **Record the values rather than whether they differed.** The proof needs only the disagreement, and a log of values is unbounded in size and carries the program's own data out of the process.
