<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Development

**Status: implemented.** Every gate, task, and tool named here exists; the catalog near the end says which milestone each arrived in.

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

`mise run check` runs every local gate, the ones that answer quickest first.
That is not the order CI runs them in, because CI runs the jobs at once and
waits for all of them while a person waits for each in turn: formatting answers
in seconds, `lint` — clippy, rustdoc, the repository gates, the fuzz crate's
own type check, spelling, TOML, workflows — in tens of them, and the suite in
minutes. Among them, `cargo xtask all` is this repository's own:

| Gate | Refuses |
| --- | --- |
| `lints` | `allow-attribute`: an `#[allow]` anywhere in the repository, tests included. `boxed-trait-object`: a `Box<dyn Trait>`. `comment`: a comment that is not documentation, the licence header, or a `rust-mutants:` annotation the engine reads |
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
names one a reader could go and read. Given the run's recording as well, it
holds the proof layers to what the run wrote down: no target a proof removed
from what could notice a mutation may then be the target that killed it, a
route that says no measured target reaches a mutation may not then run one
against it, and a route the measurement widened has to run something. The
reach layer is re-derived rather than confirmed, because a route names the
targets it removed every execution from: each of those names is held to the
targets the run reports, to the targets the same route kept, and to the
proofs that route names — and a route that removed every execution while
naming nobody is a violation, since nothing reaches a place only if somebody
was in a position to notice and did not. It reads the recording as lines of
JSON rather than through the code that wrote them, and a run recorded without
`--trace` leaves the layers `unaudited` rather than passed. This is [ADR 0004](adr/0004-proof-layers-not-budgets.md) decision 5,
which ships a proof layer only against a re-implementation that is not asked
whether it agrees with itself.

`cargo xtask engine-audit <run-directory> [--trace <recording>] [--shard
<report>…] [--ledger .rust-mutants.toml]` is the same rule for the engine's
own runs. It reads that run's `run-report-v1.json` and re-decides it in nine
layers, none of which calls the engine's code:

| Layer | Re-derives |
| --- | --- |
| `identity` | every `id`, minted again from the row's own path, rule, version, byte span, source digest, and edit; the short form as the head of the full one; the indices of the accepted and the refused as one dense catalog |
| `accounting` | every column from the rows, and the equations the columns stand in: the outcomes come to `executed`, `executed + not_run` comes to `cataloged`, and `unreached` is never larger than `not_run` |
| `score` | present exactly when the run decided something, over the columns it is a ratio of |
| `findings` | each kind as a set equality with the rows in both directions, and every finding as one that names a row |
| `expectations` | `met`, `stale`, and `unmatched` against the rows they name, and each accepted row against the one claim that accounted for it |
| `exit` | the code the run returned, from what it found |
| `merge` | the parts of one catalog: same digests, disjoint indices, and the whole they come to |
| `proofs` | every discharge against the measurement and the catalog the run kept: a target that covered the body it was discharged from, a discharge whose premises are missing, a discharged pair that then ran, the `discharged` column, and a mutant that never ran and whose reason the recording does not give |
| `trace` | every row against the recording of what actually ran: the target it names ran, its outcome is that execution's, a believed timeout repeated, instrumenting moved no line, every refusal was condemned by a round, a discharged target did not then run, an unreached route ran nothing, and every target the build produced was verified |
| `ledger` | every survivor as one the ledger accepts with a reason, and every acceptance as one the run still holds |

Its output and exit codes are `proofaudit`'s: one line per remark, a summary
line, and 0, 1, or 2. Three runs of the fixtures are committed under
`xtask/tests/testdata/engine-run-*/` and a test re-decides all three, so a
change that makes the engine disagree with itself fails here rather than in a
weekly job.

`mise run dogfood:audit` and `mise run dogfood:engine:audit` are those rules as
one command each: they run this workspace through the release build, keep the
recording, and re-decide it.

```console
$ mise run dogfood:audit
proofaudit: 20260906T052111Z-047fc6: 39 mutants and 16 targets re-decided; 0 violations, 1 unaudited
```

Where the recording does not carry enough to decide something again — which
survivors a reviewer accepted, what a reused disposition was routed under,
which regions a route was decided from —
the gate says `unaudited` and counts it apart from the violations, because
fail-closed is never turning "I cannot check this" into "this is fine", and
equally never into "this is broken". One line per remark names its layer and
its subject, a summary line closes the report, and the exit code is 0 with no
violations, 1 with them, and 2 when the run directory could not be read at
all.

## Test harness

`crates/mjutest-devkit` is test-only support shared by every crate: the
golden-file comparison, the workspace and fixture paths, the `cargo` that
built the test binary, a throwaway copy of a fixture project, and the scripted
toolchain. Each crate's `testkit` module (behind `cfg(test)` or the `testkit`
feature) holds its own fakes; production code may import neither, and
`devgates` checks.

### Two halves of the suite

A suite that starts a real `cargo` is named `toolchain_*.rs`, and every other
one is not. `mise run test:fast` is the inner loop and runs the second half in
seconds; `mise run test:slow` runs the first; `mise run test` runs both and the
doctests, which is what the pipeline runs. `cargo xtask test`'s own suite
(`xtask/tests/tasks.rs`) holds the naming to the rule, so a test that quietly
starts a toolchain cannot land in the inner loop.

### Which half a rule goes in

The split is about speed, and it decides something else as well: **whether a
mutation run can see that a rule is held.** The guards record which test
reached which mutation in the process they run in, and a test that starts the
binary in another process leaves no record there. So a mutation of the
runner's own code is never routed to a `toolchain_*` target, and an assertion
that lives only there is one the measurement cannot attribute to anything. The
rule is held; the run reports a survivor.

That happened here on 2026-09-09 to `assure/run.rs`'s first stage. A test in
`toolchain_verify.rs` asserted that a verification names each stage as it
starts, breaking the rule failed that test, and the mutation survived the run
anyway, because the route named `toolchain_watch` — the one test that drives
the runner in this process. Moving the assertion there, unchanged, killed it.

So: **a rule about what this code does goes in an in-process test**, and a rule
about what a person typing a command gets — the exit code, which stream a line
went to, the files left behind — goes in a `toolchain_*` one, where it cannot
be observed any other way. When a survivor's route names only `toolchain_*`
targets, the question to ask first is not "what test is missing" but "is the
test somewhere the measurement can see".

A whole run in this process is `mjutest_cli::run_from` with an `Environment` the
test builds, and it reaches every phase the configuration turns on. That is
what `toolchain_watch.rs` does, and it is why a `.mjutest.toml` written into a
copied fixture is the lever for a whole cluster of survivors rather than one:
`[execution] timeout` with a fixture that is slow once reaches the bound that
expires and the quiet measurement after it, `[mutation] equivalence` reaches
the layer that removes findings, `[resources]` and `[generation]` reach the
providers, and a `fuzz/fuzz_targets` directory reaches the gap a run states
about targets nobody asked it to drive. Each of those was measured as unreached
until a run in this process was configured into it.

### The scripted toolchain

`mjutest_devkit::fake_cargo` writes a script of invocations —
program, argument prefix, environment, and what to print, write, wait, and
exit with — and `crates/rust-mutants/examples/fake_cargo.rs` is the program
that answers it as `cargo`, as `rustc`, as a coverage tool, or as a test
binary. It is an example rather than a binary of the devkit because
`--all-targets`, `cargo nextest`, and `cargo llvm-cov` build the examples of a
crate under test on every platform and build no binary of a dev-dependency on
any of them. A command no entry matches exits 99 with its own command line on
stderr: a suite that forgot to script something says what it forgot.

That is what lets `crates/rust-mutants/tests/workspace.rs` hold the engine to
every refusal it can report about a toolchain — a cargo that is not a file, a
banner with no release line, metadata that is not metadata, a stream whose
second line is not a message, a build that outruns its timeout — in a fifth of
a second and with no toolchain at all.

### A copy of a fixture

`mjutest_devkit::fixture::Fixture::copy` is the tree a suite hands to the
thing it is testing. The copy is canonical, because a path a run reports has
to compare equal to the one the test holds; its temporary and cache
directories sit beside the tree, because a cache under the root would change
the tree's own digest every time a run wrote to it; and `copy_with_siblings`
puts a second fixture next to the first, which is what a path dependency that
climbs out of the tree needs.

### Line endings

`.gitattributes` says `* -text`, so a checkout is byte-exact everywhere: an
identity hashes the exact bytes of the file it was cut from, and a CRLF
checkout would change every source digest and therefore every mutant ID. The
CRLF variants the tests use are **derived** from the LF inputs by
`rust_mutants::testkit::source::crlf` rather than committed. A committed copy
is a second spelling of the same program that a checkout, an editor, or a
careless rewrite can quietly change, and then the test proves the two copies
agree rather than that the engine handles both endings. What the tests hold
the engine to: the same candidates at the same lines and columns, the same
line count after instrumenting, the same rewrite, the same fates, and
identities of its own.

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
`xtask/tests/fuzz_ledger.rs` keeps the four places that name the targets in
step: the source files, the manifest stanzas (each with `bench = false`, so
`cargo bench` never builds a sanitizer target), the README rows, and the
weekly workflow's matrix.

Being standalone is what makes them cheap to run and easy to lose: nothing in
`cargo test --workspace` compiles them, so a target can rot against an API
change and say nothing until the weekly job. `mise run fuzz:check` — part of
`mise run lint`, and a step of the CI lint job — type-checks the crate on the
toolchain the workspace uses, which is the cheapest thing that would have
noticed.

### The documentation

`docs/` is an mdbook: `mise run book` builds it into `target/book`, and
`mise run book:serve` reloads it as the pages change. Nothing is written for
the book — the pages are the ones this repository already keeps, and
`docs/SUMMARY.md` is the order to read them in.

Two gates hold the summary and the pages to each other, one in each direction.
mdbook is configured with `create-missing = false`, so a summary that names a
page nobody holds fails the build; `xtask/tests/docs.rs` refuses a page the
summary does not name. Without the second one a new page is simply absent from
the book, which nobody notices, because a book that is missing a chapter looks
exactly like a book that never had it.

### Coverage

The `coverage` job in `ci.yml` runs the suite once under `cargo llvm-cov` and
then ratchets four numbers: the workspace at 80% of regions, `rust-mutants` at
87%, `rust-mutants-cli` at 85%, and `xtask` at 80%. Each floor is a little
under what the suite reaches, so an ordinary change has room and a change that
drops a whole area does not. **A floor is raised when a suite earns it and
never lowered**; lowering one is a decision to argue for in the pull request
that does it.

The job builds the examples inside the coverage environment before running the
suite. `nextest` builds the test targets and nothing else, and part of this
suite drives a scripted `cargo` that lives in an example, so without that step
every suite that uses it fails for want of a binary rather than for a reason.
The same is true locally: `cargo llvm-cov nextest` needs
`cargo llvm-cov show-env` and a `cargo build --examples -p rust-mutants` in
between.

### Ledgers the documentation keeps

A page that names a set the code also names goes stale silently, so each such
pair is a test: `crates/rust-mutants/tests/docs_ledger.rs` holds the trace
page to `trace::EVERY_TYPE`, the architecture page's skips to
`SkipReason::ALL`, the operators page to `CANONICAL_TABLE` and its two counts,
and the limitations page to `rust_mutants::limitation::ALL`, and refuses a
page under `docs/` that does not say whether what it describes is
implemented. `crates/rust-mutants-cli/tests/docs_ledger.rs` holds the
configuration page to the keys the reader accepts in both directions, and the
JSON page to `FindingKind::ALL` and the exit codes. `xtask/tests/docs.rs`
holds `docs/ci.md` to the workflows and the jobs they hold.

### Error codes

Every error variant carries a code; `docs/errors.md` is the ledger, and a
test in each crate keeps the two equal in both directions.

## Diagnostics

Everything a run does is recordable: the runner's [trace v1](trace-v1.md),
the engine's [trace](engine/trace.md), `--keep-temp`, the diagnostics bundle
of a failed run, and the `explain` family of commands. The rule for all of it
is [ADR 0002](adr/0002-trace-is-not-evidence.md): never a claim, never a
failure, always honest about what was dropped.

Both products bundle a run the same way. `rust-mutants doctor` says what the
engine would find in this environment, and `rust-mutants diagnostics` gathers
one run — report, catalog, measurement, probe logs, recording, configuration,
doctor document, toolchain — into one directory to attach to an issue, with
the names of the variables that were set and none of their values. What to
reach for when something is wrong is [engine
troubleshooting](engine/troubleshooting.md).

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
| engine trace (every discovery decision, every validation round), goldens with CRLF variants, property tests, fuzz targets for every fail-closed parser, fixtures with fate tables, `rust-mutants explain` / `instrument --file` / `why-skipped`, runner contract tests, external-consumer contract test | seeing why the engine did what it did | M1 |
| runner trace v1 with `trace summary` and `trace diff`, diagnostics bundle, `--keep-temp` ledger, testkit (fixture repository builder, scripted workspace, `normalize_report`, helper subprocesses), report and help goldens, `xtask report-diff`, `mjutest plan --why` | seeing why a run routed what it routed | M2 |
| scripted session, route events, `mjutest explain`, accounting property tests, `mise run dogfood` | the runner on itself | M3 |
| evidence-store goldens, interruption injection, concurrent cache tests | reuse and resumption | M4 |
| `xtask proofaudit` (independent reimplementation of every proof layer), fixtures for probes and branch proofs, the kill-implies-infection soundness test | proofs before they ship | M5 |
| provider fakes with failure injection, repair rollback tests | providers and repairs | M6 |
| the nightly fuzz job | `deep-v1` | M7 |
| release consistency, install-surface job, release checklist | shipping | M8 |

## Benchmarks

`mise run bench` measures what the byte foundation, the pipeline, and the
report cost: `splice`, `flatten`, and a mutant identity in the engine's
`foundation` bench; discovery, instrumentation, cataloging, reading a coverage
export, reading the cargo configuration of a ten-deep tree, walking a match of
five hundred arms, and reading a file of `rust-mutants: skip` markers in its
`pipeline` bench; the audit and the two projections in the runner.

They are observations, never gates. Nothing fails when a number moves and no
verdict depends on one — they exist so a person can answer "did that change
make discovery slower" by looking rather than guessing, which is the same
thing [ADR 0004](adr/0004-proof-layers-not-budgets.md) asks of a proof
layer. Measured on one machine, for scale rather than for comparison:
a 200-edit splice about 7 µs, flattening one function about 13 µs, one
identity about 1.7 µs, auditing a 2000-target report about 3 µs, and writing
that report about 450 µs as JSON and 650 µs as records. Of the pipeline:
discovering a 2000-function file about 260 ms, instrumenting a 200-function
one about 18 ms, cataloging its candidates about 32 ms, and reading a coverage
export of 500 functions about 2.4 ms.

The harness is criterion with `harness = false` and a hand-written `main`:
`criterion_group!` generates an undocumented public function, and this
workspace documents everything.
