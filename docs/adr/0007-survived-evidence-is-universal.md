<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0007 — Survived evidence is a universal proposition over the reaching set

## Status

Accepted, 2026-09-05, inherited from goatest ADR 0007 (accepted 2026-09-04).
Implemented by the evidence store and the mutation-evidence rules of `assure` as the milestones deliver them.

## Context

[ADR 0004](0004-proof-layers-not-budgets.md) committed the project to speed by proof.
Reuse across runs is such a rule: what an earlier run observed about a mutant is evidence, and re-observing it establishes nothing new —
provided the claim is still a claim about this run.
A kill is reused when the recorded killer still reaches the mutant, has the same behaviour key, and passed this run's own baseline.
That covers the existential half.
Survivors are the expensive half: every reaching test of a surviving mutant runs to completion, and the result is re-derived from scratch on every run.

A survival is a different kind of claim.
"This target killed the mutant" is existential: one witness makes it true.
"No test that reaches this mutant kills it" is universal: a claim about a set, and a set that gained a member is a different set.
A condition modelled on the kill rule would be unsound in exactly the case that matters — a newly written test that kills a mutant an earlier run watched survive.

## Decision

1. **A survived record is a universal proposition over the targets that were executed.** A later run reuses it only when **every** target its own coverage routes to the mutant, after every discharge, appears in the recorded set with an equal behaviour key and passed this run's baseline.
2. **A subset is sound; a superset is not.** Reuse is refused by growth,
   never by shrinkage.
3. **Fuzz targets and resumed targets never qualify, in either direction.**
4. **A mutant the evidence cannot say nothing reaches is a claim about the package suite**, and the suite runs every prepared target, so the claim is recorded as the conjunction of every target's own behaviour key.
   Naming each target is stricter than one key over the package: a target that enters or leaves the suite refuses reuse where a package-wide key would have hidden it.
   A mutant both premises of `unreached` hold for is a claim about the code and is not reused at all.
5. **A timeout is reused fail-closed, under an existential condition**: it keeps its finding and can never remove one.
   The record names the target time ran out under as the last of its executed targets, stored in execution order.
6. **An execution that reads the repository keys the whole tree.** In Rust there is no test action log to observe with, so the selection is static and conservative: a package whose sources use directory-reading APIs (`std::fs::read_dir`, `walkdir`, `glob`, `include_dir!`, the working directory) keys the whole snapshot.
   Widening reduces reuse and never changes a verdict.
7. **Contradiction removes a record; nothing else does.** No expiry, no age.
8. **Every reuse is visible and subordinate to this run**: the route says `reused: true`, the disposition carries the provenance, the finding is raised again through *this* run's acceptances.

## Consequences

The expensive half of the mutation phase becomes reusable, under conditions strictly narrower than a kill's.
`proofaudit` counts reused routes as a class of their own rather than as measured kills.
What keeps reuse honest is the store's strictness and the end-to-end tests that run a real toolchain twice and compare verdicts.

## Alternatives rejected

Reusing a survival when *some* recorded target still reaches (unsound);
excluding repository-reading packages (ADR 0004 forbids exclusions); expiring records by age (age is not evidence); reusing a timeout as a resolution (a timeout resolves nothing); observing repository reads with `strace`,
`fanotify`, or `ptrace` (Linux-only, privileged or slow, and the attribution problem; static widening loses reuse and nothing else).
