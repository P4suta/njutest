<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# rust-mutants and cargo-mutants

**Status: implemented.** Everything claimed of this engine below is shipped; where a comparison is about a plan rather than a release, it says so on the line.

[cargo-mutants] is the mutation testing tool most Rust projects reach for, and
it is good. This page says what is different here, so that a reader can tell
whether the difference is worth anything to them. Most of the page compares
contracts. The one measured comparison below states its exact scope and does
not turn a three-mutant result into a universal benchmark claim.

[cargo-mutants]: https://github.com/sourcefrog/cargo-mutants

## The one difference everything else follows from

cargo-mutants writes one mutation into a copy of the tree, builds it, runs the
tests, and starts again for the next mutant. rust-mutants writes every
selected compilable mutation into one copy, each dormant behind a guard,
builds that once, and activates one mutant per test process through an
environment variable — the mutant schemata of the family this engine belongs to
([ocaml-mutants], [gleam-mutants], [go-mutants]).

[ocaml-mutants]: https://github.com/P4suta/ocaml-mutants
[gleam-mutants]: https://github.com/P4suta/gleam-mutants
[go-mutants]: https://github.com/P4suta/go-mutants

That is why the compiler is asked once instead of once per mutant, why a
mutant has a stable identity that survives an unrelated edit, and why a run
can route a mutant to the targets that reached it without rebuilding anything.
It is also why this engine has to be careful in ways cargo-mutants does not:
one snapshot holds every mutation at once, so a guard that changed a line
number or a runtime that touched a crate root would corrupt every later
answer, and the tests here are mostly about those invariants.

## What is different

| | cargo-mutants | rust-mutants |
| --- | --- | --- |
| Build | one per mutant | one instrumented build for the selected set |
| Mutant identity | position in the current tree | content-addressed, stable across unrelated edits |
| Which candidates are real | the build decides, per mutant | batched `cargo check` rounds decide the selected set, and every refusal is kept with the compiler's own words |
| Line numbers | shifted by the mutation | preserved, byte for byte, which is what every position in a report rests on |
| Selection | `--in-diff`, `--file`, `--regex` | `--changed`, `--include`, `--exclude`, `--package`, `--file`, `--id`, rule/family filters, `--from-report`, `--shard K/N` |
| Skipping work | `--baseline`, timeouts, `--in-diff` | content-addressed passing baseline and outcome reuse, guard and coverage routing, branch and infection proofs — no sampling |
| Unbuildable mutants | reported as a build failure | refused before execution, with the diagnostic that refused them |
| Report | `mutants.out/*.txt`, JSON | `rust-mutants/run-report` v1 with a JSON Schema, plus a Stryker projection, one offline page, and a terminal reader |
| Verdict | a count | an exit code from a stated policy, with expectations a reviewer declares and the run verifies |

## What has actually been faster

A controlled run on this repository selected the same three mutations both
engines could spell and gave both a passing baseline, one mutation job, two
build jobs, locked offline dependencies, and two libtest threads. Both found
all three. rust-mutants took 7:40.09 wall time and cargo-mutants 27.1.0 took
7:52.63; maximum RSS was 653,956 KiB and 652,392 KiB respectively. The total
lead is 12.54 seconds, or 2.65%. It establishes parity with a small lead for
that slice, not an overwhelming lead in general.

The larger win found in the same work was against this engine's own prior
implementation. A two-file selection used to validate all 12,743 catalog
candidates and took 5:40:14. Selection-aware witness generation,
instrumentation, and validation brought a comparable run to 17:59: 346 times
faster in validation and 18.9 times faster end to end. Exact repeated
instrumented baselines now start no baseline target processes at all. The
dated commands, digests, outcomes, and the distinction between measured and
inferred time are recorded in [Current boundaries and evidence].

[Current boundaries and evidence]: ../limitations.md#speed-regression-closed-by-this-change

## What cargo-mutants does that this does not

- It is released, widely used, and documented for a general audience. This is
  pre-1.0 and its contracts are still moving.
- It works without the instrumentation this engine writes, so anything this
  engine cannot instrument is something cargo-mutants can still mutate. What
  that is now is narrow and named: a crate the host cannot lend `std` to (one
  supplying a `#[panic_handler]` or a `#[global_allocator]`, `#![no_main]`, or
  edition 2015), a fragment pasted in by `include!` where an expression goes,
  and what a procedural macro expands to — the macro decides that during the
  build, and a mutation is activated per test process, so the two never meet.
  An ordinary `#![no_std]` crate, a proc-macro crate's own tests, and a file
  `include!` pastes in at item position are all measured. The skips are stated
  in every report rather than passed over silently, but stated is not the same
  as done.
- Its per-mutant build isolates a mutation completely. One snapshot cannot: a
  test that writes into the tree it is being measured in is a limitation this
  engine reports and cargo-mutants does not have.

## What this does that cargo-mutants does not

- **Proofs rather than budgets.** A mutant is not run against a target whose
  measured run never reached it, a branch proof discharges a target that never
  took the branch, and an infection proof discharges a test that could not have observed
  a return replacement. Every one of these is evidence the run already holds;
  none of them is a time limit or a sample.
- **Paired confirmation.** A kill is believed only when the original passes
  now and the kill reproduces.
- **Stable identities.** A mutant's identity is a hash of what it is, so an
  expectation a reviewer wrote survives an edit elsewhere in the file.
- **A catalog you can read before anything runs.** `list`, `catalog --json`,
  `why-skipped`, `explain`, and `instrument --file` each answer a question
  about what would be measured, without measuring it.
- **Answers a script can act on.** A JSON Schema for the report, a documented
  exit policy, `--shard K/N` with a `merge` that refuses parts that disagree,
  and an outcome cache keyed on the tree and the rules rather than on a
  timestamp.

## When to use which

Reach for cargo-mutants when you want mutation testing today on a workspace
you have not thought about instrumenting, or when its per-mutant isolation is
worth the builds to you.

Reach for rust-mutants when the answer has to be auditable — when somebody
will ask *why* a mutant was not run and "the budget ran out" is not an
acceptable answer.
