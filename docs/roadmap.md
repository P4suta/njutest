<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Roadmap

**Status: M0 done.** The user's decisions: one workspace, the engine first,
the engine a standalone product too, every milestone completed, test-driven
throughout, developer infrastructure first.

Each milestone has two halves — the feature and the developer infrastructure
that lets it be seen, tested, and audited — and both are completion criteria.

| # | Milestone | Feature | Developer infrastructure | Done when |
| --- | --- | --- | --- | --- |
| M0 | Scaffold | workspace, lints, gates, CI, contracts, both command-line skeletons, the public API crate | devkit, error-code ledger, `xtask` gates from tests, `bacon`, `doctor`, `CLAUDE.md` | `mise run check` green |
| M1 | rust-mutants engine | stable IDs, byte splicing, snapshot and owners, process supervision, syntactic discovery, guards and runtime, compiler-validated acceptance, execution, the library API, `list`/`catalog`/`run` | engine trace, goldens, property tests, fuzz targets, fixtures, explain commands, contract tests | `fixture-killable`'s fates are fixed by tests |
| M2 | `mjutest verify` baseline | configuration, targets, per-target baseline under coverage, regions, report v1, verdicts, `plan`/`doctor`/`init` | trace v1, diagnostics, testkit, `reportdiff`, `tracesummary` | a fixture yields a report with regions and a trace |
| M3 | Mutation phase | routing, paired confirmation, accounting, acceptances, `explain`/`replay`/`accept`, HTML/SARIF/JUnit | scripted session, route events, dogfood | mjutest reaches a verdict on itself |
| E2 | rust-mutants standalone | `.rust-mutants.toml`, run report v1, exit policy, timeout retry, score, expectations | engine dogfood, report goldens, interrupt contract | rust-mutants scores itself |
| M4 | Identity, cache, evidence | digests, behaviour keys, exact cache, checkpoint, evidence reuse, `--changed`, unsafe inventory, build-cache gc | interruption injection, reuse goldens | a second run reuses the first |
| M5 | Proofs | probe tree, infection log, witness tree, branch proof, discharges, dashboard | `proofaudit` with zero violations on a dogfood recording | kill implies infection on `fixture-probeable` |
| E3 | Engine incremental | outcome cache, `--changed`, sharding and merge, coverage-guided selection | determinism and concurrency tests | a sharded run equals a whole one |
| M6 | Resources and repair | providers, candidates, `fix --apply`, retention | provider fakes, rollback tests | the goatest provider suite passes here |
| M7 | `deep-v1` and fuzz | Miri, sanitizers, cargo-fuzz, corpus promotion | nightly jobs, fuzz fixture | weak test → survivor → fuzz → corpus → fresh kill |
| E4 | Engine reports | Stryker projection, offline HTML, TUI, `doctor-v1` | schema validation, TUI snapshots | a Stryker-valid report |
| M8 | Release | release workflow, SBOM, provenance, comparison document | release gates, install-surface job | v0.1.0 |
