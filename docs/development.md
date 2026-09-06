<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Development

This document describes the infrastructure for working on mjutest and
rust-mutants themselves. The other pages under `docs/` describe the tools;
this one describes the tests, gates, and diagnostics that hold them to their
contracts. Setup, pull request rules, and source conventions live in
[CONTRIBUTING.md](../CONTRIBUTING.md).

Developer infrastructure comes first. Every milestone of the
[roadmap](roadmap.md) carries its tests, traces, gates, and diagnostics as
completion criteria, and speed never wins over them.

## TDD protocol

Development is test-driven, in three steps of one change:

1. **Red.** Write the test against the behaviour, not the implementation, and
   watch it fail for the stated reason. Paste that output in the pull
   request. A test that passes before the change is not evidence.
2. **Green.** Make it pass with the smallest change that is honest about the
   contract. Fail-closed behaviour is part of the contract, not an error path
   to add later.
3. **Refactor.** Remove the duplication the change introduced, with the suite
   green throughout.

Evidence is doubled where it can be: a pure function has unit tests and a
property test; a boundary (a subprocess, the filesystem, cargo) has a test
against a fixture project; a contract (JSON, a command line, an exit code, a
trace) has a golden. The suite is itself mutation-tested twice — weekly by
cargo-mutants, and from M3 by `mise run dogfood` — and a survivor is a test to
write or an acceptance to record with a reason, never something to leave.

## Gates

`mise run check` runs every local gate in the order CI runs them. Among them,
`cargo xtask all` is this repository's own:

| Gate | Refuses |
| --- | --- |
| `devgates` | a seam the ledger `xtask/seam_allowlist.txt` does not name, and a ledger line the tree no longer has: `static mut`, a `static` with interior mutability, `thread_local!`, `#[cfg(test)]` outside a `mod tests`, a read of the process environment or an exit outside `main.rs`, an import of test support from production code ([ADR 0001](adr/0001-seam-policy.md)) |
| `deps` | an internal dependency in the wrong direction ([ADR 0012](adr/0012-one-workspace-two-products.md)) |
| `fixtures` | a fixture project without a `[workspace]` table, a committed `Cargo.lock`, the SPDX header, or with a dependency that is not a path inside itself |
| `release-check` | a workspace version that disagrees with the release manifest, or a member that does not inherit it |

The gates are also tests (`xtask/tests/gates.rs`), so `cargo test` refuses
the same things.

`cargo xtask proofaudit <run-directory> [--trace <recording>]` stands apart from
`all`, because it is about one completed run rather than about the tree. It reads that run's
`mjutest-assurance-report-v1.json` and decides again, with code that never
calls the runner's, whether each verdict is the one the recorded evidence
supports: whether the columns say what the records they summarise say and add
up the way the [assurance contract](assurance-contract.md) states, whether
every kill names a target this run itself saw pass on the original tree,
whether the mutations nothing noticed and the `surviving-mutant` findings are
the same set, and whether every disposition read back from an earlier run
names one a reader could go and read. Given the run's recording as well, it re-derives the one thing the proof layers
can be held to from what a run wrote down: no target a proof removed from what
could notice a mutation may then be the target that killed it. It reads the
recording as lines of JSON rather than through the code that wrote them, and a
run recorded without `--trace` leaves the layers `unaudited` rather than
passed. This is [ADR 0004](adr/0004-proof-layers-not-budgets.md) decision 5,
which ships a proof layer only against a re-implementation that is not asked
whether it agrees with itself.

Where the recording does not carry enough to decide something again — which
survivors a reviewer accepted, what a reused disposition was routed under —
the gate says `unaudited` and counts it apart from the violations, because
fail-closed is never turning "I cannot check this" into "this is fine", and
equally never into "this is broken". One line per remark names its layer and
its subject, a summary line closes the report, and the exit code is 0 with no
violations, 1 with them, and 2 when the run directory could not be read at
all.

## Test harness

`crates/mjutest-devkit` is test-only support shared by every crate: the
golden-file comparison, the workspace and fixture paths, and the `cargo` that
built the test binary. Each crate's `testkit` module (behind `cfg(test)` or
the `testkit` feature) holds its own fakes; production code may import
neither, and `devgates` checks.

### Golden files

`golden(path, got)` compares recorded bytes against a file, byte for byte,
and reports one failure that names the file and shows a unified diff. Without
`UPDATE_GOLDEN=1` the comparison is read-only, and a missing file is a
failure rather than a silent first recording. `TRYBUILD=overwrite` is the
same switch for the compile-error goldens of the attribute macros.

### Fixture projects

`fixtures/` holds independent cargo projects the suites drive with a real
`cargo`, offline. Each states its purpose in a `README.md` and, where it
exists to have a known fate under mutation, a table of every mutant and its
expected outcome. See [fixtures/README.md](../fixtures/README.md).

### Fuzz targets

`fuzz/` is a standalone cargo-fuzz crate (nightly, sanitizer) with one target
per fail-closed parser or byte transformation of the engine; each target
states one property in its doc comment and `fuzz/README.md` lists them.
`mise run fuzz:smoke` runs every target briefly; the `fuzz` workflow does the
same on a pull request that touches the engine and spends real time weekly.
A crash reproducer worth keeping becomes a regular test.

### Error codes

Every error variant carries a code; `docs/errors.md` is the ledger, and a
test in each crate keeps the two equal in both directions.

## Diagnostics

Everything a run does is recordable: the runner's [trace v1](trace-v1.md),
the engine's [trace](engine/trace.md), `--keep-temp`, the diagnostics bundle
of a failed run, and the `explain` family of commands. The rule for all of it
is [ADR 0002](adr/0002-trace-is-not-evidence.md): never a claim, never a
failure, always honest about what was dropped.

`mjutest trace summary` is where a person asks where a run went. It counts the
events by type, times every stage the run said it had reached, counts the
commands by program, says how many executions each proof removed, and names the
slowest commands. A run records the engine's own trace in a directory beside
its own, and the summary reads that too: the engine does most of a run — the
snapshot, the instrumentation, the validation rounds, the builds — so a summary
that read only the runner's would leave the larger part of every run
unaccounted for. Both are read with one command:

```console
mjutest verify --trace
mjutest trace summary
```

The numbers are the ones to optimise against, and the rule for acting on them
is [ADR 0004](adr/0004-proof-layers-not-budgets.md): a run that is too slow is
a run missing a proof, or doing work nothing reads — never a run that should
measure less.

## The catalog

The developer-facing infrastructure, and the milestone it arrives in:

| Means | For | Arrives |
| --- | --- | --- |
| devkit (golden, paths), error-code ledger, `cargo xtask` gates, `bacon`, `mise run doctor`, `CLAUDE.md` | the inner loop and the ratchets | M0 |
| engine trace (every discovery decision, every validation round), goldens with CRLF variants, property tests, fuzz targets for every fail-closed parser, fixtures with fate tables, `rust-mutants explain` / `instrument --print` / `why-skipped` / `validate --explain`, runner contract tests, external-consumer contract test | seeing why the engine did what it did | M1 |
| runner trace v1 with `trace summary` and `trace diff`, diagnostics bundle, `--keep-temp` ledger, testkit (fixture repository builder, scripted workspace, `normalize_report`, helper subprocesses), report and help goldens, `xtask report-diff`, `mjutest plan --why` | seeing why a run routed what it routed | M2 |
| scripted session, route events, `mjutest explain`, accounting property tests, `mise run dogfood` | the runner on itself | M3 |
| evidence-store goldens, interruption injection, concurrent cache tests | reuse and resumption | M4 |
| `xtask proofaudit` (independent reimplementation of every proof layer), fixtures for probes and branch proofs, the kill-implies-infection soundness test | proofs before they ship | M5 |
| provider fakes with failure injection, repair rollback tests | providers and repairs | M6 |
| the nightly fuzz job | `deep-v1` | M7 |
| release consistency, install-surface job, release checklist | shipping | M8 |

## Benchmarks

`mise run bench` measures what the byte foundation and the report cost:
`splice`, `flatten`, and a mutant identity in the engine; the audit and the
two projections in the runner.

They are observations, never gates. Nothing fails when a number moves and no
verdict depends on one — they exist so a person can answer "did that change
make discovery slower" by looking rather than guessing, which is the same
thing [ADR 0004](adr/0004-proof-layers-not-budgets.md) asks of a proof
layer. Measured on one machine, for scale rather than for comparison:
a 200-edit splice about 7 µs, flattening one function about 13 µs, one
identity about 1.7 µs, auditing a 2000-target report about 3 µs, and writing
that report about 450 µs as JSON and 650 µs as records.

The harness is criterion with `harness = false` and a hand-written `main`:
`criterion_group!` generates an undocumented public function, and this
workspace documents everything.
