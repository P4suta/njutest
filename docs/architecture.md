<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Architecture

**Status: scaffold.** The workspace, the gates, the command-line contracts of
both binaries, and the public API crate exist. Every phase described below is
the design; each page under `docs/` carries its own status line, and nothing
here should be read as a description of working software until its status
says so. The order of work is in [roadmap](roadmap.md).

mjutest is an orchestrator, not a replacement testing framework. Its core
pipeline is:

```text
CLI/config
   │
   ├─ exact repository + dependency + environment identity
   ├─ cargo metadata / cargo test --no-run baseline build
   ├─ per-target baseline under coverage instrumentation ── changeset routing
   ├─ resource leases
   ├─ soundness (unsafe inventory; Miri under deep-v1)
   ├─ rust-mutants catalog + infection probe pass
   ├─ region-routed mutant execution + paired confirmation
   ├─ targeted fuzzing / generation candidate validation
   └─ report v1 + exact cache/checkpoint
```

The `assure` module coordinates a round. The `cargo` module discovers native
targets and coverage, `mutation_bridge` freezes the rust-mutants contract, and
`evidence` creates content identities and the impact graph. Providers run as
subprocesses behind strict JSON protocols; core contains no network client.

## Two products

| | `rust-mutants` | `mjutest` |
| --- | --- | --- |
| Crates | `crates/rust-mutants` (library), `crates/rust-mutants-cli` (binary) | `crates/mjutest-cli` (binary and library), `crates/mjutest` (public API), `crates/mjutest-macros` |
| Family | ocaml-mutants → gleam-mutants → go-mutants → rust-mutants | goatest → mjutest |
| Role | one instrumented snapshot, every mutant behind a guard, one environment variable per test process | verdicts from coverage-routed mutation with proofs, not budgets |
| Contracts | [engine/architecture](engine/architecture.md), [engine/operators](engine/operators.md), [engine/json-schema](engine/json-schema.md) | [assurance contract](assurance-contract.md), [report v1](report-v1.md), [checkpoint v1](checkpoint-v1.md), [trace v1](trace-v1.md) |

The dependency direction is fixed and gated ([ADR 0012](adr/0012-one-workspace-two-products.md)):
the runner depends on the engine, never the reverse.

## What is Rust-specific

The thesis — [proof layers, not budgets](adr/0004-proof-layers-not-budgets.md)
— and the invariants — a read-only source workspace, a disposable snapshot,
stable content-addressed mutant identities, strict configuration, honest
reports — are inherited. Five decisions are Rust's own:

- **Type facts come from the compiler, not from a type checker library.**
  Discovery is syntactic; acceptance is `cargo check` with diagnostics
  attributed to the mutant that caused them; the probes rely on inference and
  trait bounds; the branch proof asks a witness tree
  ([ADR 0008](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)).
- **`soundness` replaces `race`.** Safe Rust has no data races to detect; the
  residual fault class is `unsafe`, inventoried under `standard-v1` and run
  under Miri in `deep-v1` ([ADR 0009](adr/0009-soundness-replaces-race.md)).
- **Target directories are the cache layers.** Compiling commands get
  `--target-dir` into a machine-wide base layer; every process that runs tests
  gets `CARGO_TARGET_DIR` into the run's scratch
  ([ADR 0010](adr/0010-target-directories-are-the-cache-layers.md)).
- **The mutant runtime lives at the end of each instrumented file**, so no
  crate root is edited and no line moves
  ([ADR 0011](adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)).
- **One workspace holds both products**, so a proof is one pull request
  ([ADR 0012](adr/0012-one-workspace-two-products.md)).

## Mutation routing

Mutation routing reads the baseline coverage at region granularity: a mutant
is run by the targets whose executed regions contain its start position,
cheapest target first. A position that cannot be placed in a region widens
back to every target that executed the file, and a position no target
executed is left to its package suite. Where rust-mutants proves a mutation
can only narrow the condition of a branch, the targets that never entered the
body that branch gates are discharged from the reaching set instead of
executed. Between preparing the catalog and executing it, a `probe` phase
measures infection: rust-mutants builds a second instrumented tree in which
each site it has a probe form for records whether the mutated value would
have differed, and each baseline target runs against it once. A measured
target that left a probed mutant out of its infections is discharged with
reason `never-infected`. Everything the measurement does not cover is kept.
See [the assurance contract](assurance-contract.md).

## Across runs

The mutation phase keeps a store of what it established about each mutant,
`.mjutest/cache/mutation-evidence-v1.json`. A kill is existential and is
reused when the recorded killer still reaches the mutant, has the same
behaviour key, and passed this run's own baseline. A survival is universal
and is reused only when every target that reaches the mutant now is one the
recording run ran against it under the same key. A timeout keeps its finding.
See [ADR 0007](adr/0007-survived-evidence-is-universal.md).

## Everything a run writes

Every byte a run writes outside the repository goes below one scratch
directory it makes for itself and removes when it ends, `mjutest-run-*`
under the configured temporary root, with an owner pair — an advisory lock
held open for the whole run and an `mjutest-temp-owner-v1` marker
([ADR 0006](adr/0006-every-temporary-directory-has-an-owner.md)). Verification
is read-only: a killing fuzz input or a generated test is stored as an
isolated candidate, and only `fix --apply` changes the worktree.

Reports are the durable boundary. Before a report can advance a latest index,
it must satisfy the scope/verdict rules, timing and toolchain requirements,
mutant inventory equations, acceptance linkage, and cache provenance checks.

## Layers

```text
main.rs            composition root: arguments, streams, environment, executable, cache and temp roots
   ↓
cli                clap parsing, help, exit codes; nothing about running
   ↓  Service trait
app                report persistence, doctor, fix, cache maintenance, trace reading
   ↓  assure::Dependencies (a table of the run's collaborators, passed, never global)
assure             one round: the phases in order, each a function over its inputs
   ↓
cargo, mutation_bridge, evidence, build_cache, cache, checkpoint, trace, ui,
repair, resource, provider, temp_owner, kept_ledger, retention, process_tree,
advisory_lock, environment, test_args, report, config
```

Every seam is an argument ([ADR 0001](adr/0001-seam-policy.md)); the gate
`cargo xtask devgates` refuses the alternative.
