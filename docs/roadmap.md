<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Roadmap

**Status: M0 to M10 and E2 to E4 are done; M11 has one item left.** What remains of
M8 is the tag itself, which is a decision rather than a change: see
`docs/release.md`. The user's decisions: one workspace, the engine first,
the engine a standalone product too, every milestone completed, test-driven
throughout, developer infrastructure first.

Each milestone has two halves — the feature and the developer infrastructure
that lets it be seen, tested, and audited — and both are completion criteria.

| # | Milestone | Feature | Developer infrastructure | Done when |
| --- | --- | --- | --- | --- |
| M0 ✓ | Scaffold | workspace, lints, gates, CI, contracts, both command-line skeletons, the public API crate | devkit, error-code ledger, `xtask` gates from tests, `bacon`, `doctor`, `CLAUDE.md` | `mise run check` green |
| M1 ✓ | rust-mutants engine | stable IDs, byte splicing, snapshot and owners, process supervision, syntactic discovery, guards and runtime, compiler-validated acceptance, execution, the library API, `list`/`catalog`/`run` | engine trace, goldens, property tests, fuzz targets, fixtures, explain commands, contract tests | every fixture's fates are fixed by tests |
| M2 ✓ | `mjutest verify` baseline | configuration, targets, per-target baseline under coverage, regions, report v1, verdicts, `plan`/`doctor`/`init` | trace v1, diagnostics, testkit, `report-diff`, `trace summary`/`diff`, fuzz targets, benchmarks | a fixture yields a report with regions and a trace |
| M3 ✓ | Mutation phase | routing, paired confirmation, accounting, acceptances, `explain`/`replay`/`accept`, HTML/SARIF/JUnit | scripted session, route events, dogfood | mjutest reaches a verdict on itself |
| E2 ✓ | rust-mutants standalone | `.rust-mutants.toml`, run report v1, exit policy, timeout retry, score, expectations | engine dogfood, report goldens, interrupt contract | rust-mutants scores itself |
| M4 ✓ | Identity, cache, evidence | digests, behaviour keys, exact cache, checkpoint, evidence reuse, `--changed`, unsafe inventory, build-cache gc | interruption injection, reuse goldens | a second run reuses the first |
| M5 ✓ | Proofs | probe tree, infection log, witness tree, branch proof, discharges, dashboard | `proofaudit` with zero violations on a dogfood recording | kill implies infection on `fixture-probeable` |
| E3 ✓ | Engine incremental | outcome cache, `--changed`, sharding and merge, coverage-guided selection | determinism and concurrency tests | a sharded run equals a whole one |
| M6 ✓ | Resources and repair | providers, candidates, `fix --apply`, retention | provider fakes, rollback tests | the goatest provider suite passes here |
| M7 ✓ | `deep-v1` and fuzz | Miri, sanitizers, cargo-fuzz, corpus promotion | nightly jobs, fuzz fixture | weak test → survivor → fuzz → corpus → fresh kill |
| E4 ✓ | Engine reports | Stryker projection, offline HTML, TUI, `doctor-v1` | schema validation, TUI snapshots | a Stryker-valid report |
| M8 ✓ | Release | release workflow, SBOM, provenance, comparison document | release gates, install-surface job | v0.1.0 |
| M9 ✓ | The contracts and the code, said the same way | a timeout is a finding, an acceptance answers only for a mutation nothing noticed, the infection proof fires, a scoped run builds its own packages, mutations are measured `[execution] jobs` at a time, `replay` puts one finding back to the tests, a mutation nothing reached is unreached only where the evidence says so, a library's documentation is a target | typed `route`/`mutant-exec`/`probe-exec` records, every stage timed, `trace summary` naming the slowest commands and reading the engine's recording, `proofaudit` holding the layers to the kills | every page describes what the code does, and `proofaudit --trace` re-decides a real recording with no violations |
| M10 ✓ | Equivalent mutants, proved | `[mutation] equivalence`: the compiler renders a mutation identically or it does not, and a run says `equivalent` only where the tests ran the position | `rust-mutants equivalence` over a whole catalog, `fixture-equivalent`, ADR 0013 | a mutation nothing could notice is not a finding, and one in code nothing calls still is |
| M11 | The Rust-shaped gaps | fifty-one operators, mutation inside the assertion macros, the files `include!` pastes in, `#![no_std]` crates, a proc-macro crate's own tests | five fixtures with fate tables, the rule-order guard, the skip reasons that are now emitted rather than named | every limitation the docs list is one a report carries |

## What M11 has left

Doctests run and are classified, and they route no mutant: a mutation only a
documented example would kill is reported as surviving, and
`doctests-not-routed` says so. Routing them means running each one as a
process of its own and reading its coverage, which `--test-runtool` makes
possible on stable and which is the largest single piece of the milestone.

## What M9 closed

Each of these was a page promising something the code did not do, found by
reading the two against each other: a timeout that raised no finding, an
acceptance that answered for an outcome nobody could sign off, an infection
proof that never fired, a scoped run that built every package, `[execution]
jobs` that nothing read, a `replay` command that was in the help and was not a
command, a documentation target nothing ran, and a mutation reported as
reaching nothing on evidence that said no such thing.

A loaded machine is still a different machine from the one a budget was
calibrated on, which is why an expired budget now buys one measurement with
the machine to itself before a run decides that time really ran out.
