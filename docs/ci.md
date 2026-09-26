<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# CI usage

**Status: implemented.** Every job named here exists in `.github/workflows/`.

A repository will run njutest from a tagged release or a checkout:

```yaml
jobs:
  assurance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools
      - run: cargo install njutest --locked
      - name: Pull-request scope
        if: github.event_name == 'pull_request'
        run: njutest verify --changed-from=origin/${{ github.base_ref }} --ui=plain
      - name: Full main scope
        if: github.event_name != 'pull_request'
        run: njutest verify --ui=plain
      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: njutest-reports
          path: reports/
```

To diagnose a run that only misbehaves on the runner, set `NJUTEST_TRACE: '1'` on the verify step and upload `.njutest/trace/` with the reports.

## The workflows of this repository

| Workflow | Jobs | When |
| --- | --- | --- |
| `ci.yml` | the test matrix (macOS 26 in two halves, macOS 15 on what meets the kernel, Windows whole), lint (fmt, clippy, rustdoc, the `cargo xtask` gates, typos, taplo, actionlint, committed), cargo-deny, cargo-audit, the coverage ratchet which is also the Linux suite, the `book` build, `soundness`, `action-smoke`, `action-smoke-rust-mutants`, and `ci-success` which gathers them | every pull request, weekly on `main`, and on request |
| `main.yml` | `tested-tree` proves that the tree a push to `main` brings is the tree a pull request head passed `ci-success` with | every push to `main` |
| `mutation.yml` | `cargo-mutants` over each package | weekly, and on request |
| `dogfood.yml` | `whole` runs the engine over its own catalog in one job through the `rust-mutants` action, checks the recording, and re-decides the run against the ledger | weekly, and on request |
| `fuzz.yml` | every fuzz target for a fixed time | weekly, and on an engine pull request |
| `release-plz.yml`, `release.yml` | the release train | every push to `main`, and on a tag |
| `codeql.yml` | CodeQL over Rust and over the workflows, with the `security-extended` queries; `CodeQL required` is the one name a protection rule asks for | every push and pull request, and weekly |
| `dependency-review.yml` | what a pull request adds to the dependency graph, refused at moderate severity in any scope | on a pull request |
| `scorecard.yml` | the OpenSSF Scorecard of this repository, published | every push to `main`, and weekly |

### What a pull request is asked, and why only that

A merge is a squash of a pull request that was up to date with `main`, which the protection rule requires, so the tree `main` receives is the tree that pull request's head was tested as.
Running `ci.yml` again on the push asked every question a second time of the same bytes, about three and a half runner-hours each time, and `main.yml`'s `tested-tree` proves the identity instead: it reads the pushed commit's tree and the merged head's, and the head's `ci-success`, and fails when either differs.
The whole of `ci.yml` still runs on `main` weekly, where a runner image can change under unchanged code, and on request.

Within a pull request's run, each row answers for something no other row does.
Linux runs the whole suite once, instrumented, in `coverage`, which has to run all of it to measure it; the uninstrumented Linux row it replaced asked the same questions again.
`macos-26` is the slowest runner and set how long every pull request waited, so it runs the suite in two halves nextest chooses by hash, each on its own runner.
`macos-15` is there for its kernel, where `proc_listpgrppids` reports an empty process group after a timeout, so on a pull request it runs the in-process suite and the suites that drive a real cargo and answer with a process group, a signal, a crash, a durable write or a schedule; weekly it runs everything.
Windows runs whole.

The `book` job builds `docs/` with mdbook, which refuses a summary that names a page the repository does not hold; `cargo test -p xtask --test suite docs::` refuses the other direction, a page the summary does not name.
A page that neither side notices is one a reader of the book cannot reach.

`codeql.yml`, `dependency-review.yml`, and `scorecard.yml` answer about the supply chain rather than about this code: what a query finds in it, what a change adds to the graph below it, and what the posture of the repository looks like from outside.
`cargo deny` and `cargo audit` in `ci.yml` ask the same question of the graph that is already here, on every push; dependency review asks it of the difference, and says so on the pull request.
Secret scanning, push protection, Dependabot security alerts, and private vulnerability reporting are repository settings rather than workflows, and are on.
Renovate opens dependency and security-fix pull requests from the shared P4suta configuration.

Every workflow declares `shell: bash` as its default, which GitHub runs as `bash -e -o pipefail` on each of the three platforms, and no step or composite action names another shell (`cargo test -p xtask --test workflows`).
Without that default a Linux or macOS step runs in `bash -e`, where `njutest verify | tee out` has the status of `tee`, and a Windows step runs in PowerShell, which goes on past a native command that failed.
The first let the `soundness` job read on after an exit code nobody had looked at; the second once reported a failing test as a cancelled job forty-five minutes later.

The required checks are the ones `ci-success` gathers.
`mutation.yml` and `dogfood.yml` are the two independent measurements of how strong this suite is, and neither gates a pull request: a survivor is a test to write or an acceptance to record with a reason, which is work to schedule rather than a push to block.

## Dogfooding the engine

`dogfood.yml` runs `rust-mutants` over its own engine in one job, through `.github/actions/rust-mutants`, the action another repository uses, with `--trace`.
The store the action restores and saves is what lets one job hold the whole catalog: a run reads back every answer the last one established that still holds, and a run the clock ends keeps what it established, so the next one continues from there.
The job then:

1. asks `rust-mutants trace check` whether the recording is complete,
2. runs `cargo xtask engine-audit <run> --trace <run>/trace --sites --ledger .rust-mutants.toml`, which re-mints every identity,
   re-tallies every column, re-derives every discharge and every carried answer from the evidence the run kept, holds every row to the recording of what actually ran, and refuses a survivor the ledger does not accept,
3. says what moved since the last complete run, with `xtask report-diff` against that run's own artifact.
   There is nothing to compare on the first run of the workflow, and a diff against nothing is not a failure; a diff that cannot be read is.

The first two steps run locally as `mise run dogfood:engine:audit`.

## Mutation testing somebody else's project

`rust-mutants` is a product in its own right, and this is what a project that uses it puts in its own workflow: one job, with no matrix to write and nothing to put back together.

```yaml
name: mutation
on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read
  security-events: write

jobs:
  mutation:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools
      - uses: P4suta/njutest/.github/actions/rust-mutants@main
        with:
          args: --locked
```

On a pull request the action measures what the change touched, against the pull request's base; on a push it measures the whole catalog.
It runs as many executions at once as the runner has cores (`--jobs all`), writes the step summary, annotates every survivor on a changed line, uploads the survivors to code scanning, and exits with the run's verdict.

Four things are worth saying about it.

**The gate is the verdict, not a percentage.** There is no threshold input; see [ADR 0004](adr/0004-proof-layers-not-budgets.md).
The gate reads exactly the report the run printed, so a run that wrote none is never judged by an older one.

**Speed comes from less work, not more machines.** The action restores the store earlier runs left and saves it again, so a push to `main` reads what the last one established and a pull request reads what its base established.
An answer is read back only where a proof says it still holds: under the exact tree, or carried across an edit its executions never entered ([ADR 0041](adr/0041-an-answer-carries-across-an-edit-it-never-entered.md)).
A mutant's process ends at its first failing test, and the target that killed a mutant before is asked first.
Splitting the catalog across a matrix lowers no mutant's cost and multiplies the jobs with the size of the change, so the action does not.

**What it does not carry is the build.** The compiled tree is the engine's, under the temporary directory, and every run of the action compiles it once.

**The inputs.** `args` passes further arguments to `rust-mutants run`, `changed-from` names another revision to measure against (empty measures the whole catalog), `upload-sarif: "false"` skips code scanning, `working-directory` names a workspace below the checkout, and `version` installs a given release.
Its outputs are `verdict` (`detected`, `found`, `failed` or `interrupted`, or `untouched` when the change touched no Rust file the configuration measures, which is a complete answer and passes), `report` and `sarif`.

## The contract that promises interpretation

`soundness` is the only job with Miri on it.
Everywhere else — every developer's machine that has not installed it, and every other job here — the whole of `deep-v1` is the refusal it is supposed to be (`NJ7001`), and a refusal is not the promise kept.
So one job installs the interpreter, verifies a fixture under the contract, and reads back that the run says it interpreted the suite.

It is a separate job rather than part of the matrix because Miri builds its own standard library the first time it runs, which costs about twenty seconds and has nothing to do with the three platforms.

## Carrying answers between machines

The cache a run reads back lives on the machine that filled it, and a hosted runner is a fresh machine every time.
`njutest cache --export FILE` writes every answer this machine holds, one JSON document to a line, and `njutest cache --import FILE` reads them into another machine's store.
Both say how many moved, so a job that carried nothing says so rather than succeeding silently.

```yaml
- uses: actions/cache@v4
  with:
    path: answers.jsonl
    key: njutest-answers-${{ github.sha }}
    restore-keys: njutest-answers-
- run: if [ -f answers.jsonl ]; then njutest cache --import answers.jsonl; fi
- run: njutest verify --locked
- run: njutest cache --export answers.jsonl
  if: always()
```

The import is skipped only when there is no file yet, on the first run of a repository, and the export runs whatever the verdict; nothing here fails quietly.
An answer arriving from another machine is held to exactly what a run of this one would keep it to — the identity it is filed under, and the audit every durable report must satisfy — because that rule lives in one place and a second copy of it would be a second chance to write it more loosely.
A line that is not an answer refuses the import and names the line (`NJ8004`); an entry this machine cannot read back refuses the export and names the entry, since copying an answer nobody can check makes one broken answer into two.

**What this does not carry is the build.** The compiled tree lives under the temporary directory, keyed to the workspace root, and it is the engine's rather than the runner's; `rust-mutants cache` reports that temporary root and the target directories it holds.
A matrix that wants to compile once gives every leg a stable `TMPDIR` and caches the `rust-mutants-target-*` directory below it.
Every leg must use the same platform and toolchain for those artifacts to be usable at all.

## Using it from another repository

`.github/actions/njutest` is a composite action that installs a published release, verifies the workspace, and hands the findings to code scanning.
It is what a project that is not this one runs.

```yaml
permissions:
  contents: read
  security-events: write

jobs:
  assure:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: <owner>/njutest/.github/actions/njutest@v0.1.0
        with:
          args: --locked
```

`version` pins the release, `args` is what follows `njutest verify`,
`working-directory` picks a workspace inside the checkout, and `upload-sarif` turns the code-scanning step off for a repository that has no `security-events: write`.
The action's outputs are `verdict`, and `report`, `sarif` and `junit`, the paths of the files the run wrote.
Each path is one `njutest verify` printed as a `REPORT`, `SARIF` or `JUNIT` record from the list of files it sealed into the run directory, so the action never rebuilds a path the run did not promise.

The step ends with the verdict's own exit code, so a `DEFECT` fails the job —
**after** the findings have been uploaded.
A run that swallowed the status to upload first, or uploaded nothing because the status was non-zero, would leave a person reading a log instead of the findings themselves.

`cargo binstall njutest` is what installs it, from the archive `release.yml` publishes, so the action does not compile this workspace inside somebody else's job.
`cargo njutest verify` works too, wherever the binary is on the path: the same program answers to the name cargo looks for.

A workflow that has already put `njutest` on the path keeps it: the action installs nothing unless `version` names a release or the binary is absent.
A job that built the commit under test, restored a cached binary, or installed from somewhere else has said which one it wants, and installing over it would answer a question nobody asked.

That is also what makes the action testable here.
`action-smoke` builds this workspace's own `njutest`, puts it on the path, and runs the action against `fixtures/fixture-assured` the way another repository would, checking that the `verdict` output is the one the run reached and that every other output names a file that exists, so an output added later is held to the same check.
`action-smoke-rust-mutants` does the same for `.github/actions/rust-mutants`, over a tree whose tests notice every mutant, which must say `detected` and pass, and over one that leaves a finding, which must say `found` and fail the step.
What it does not exercise is the install itself, which needs a published release; until there is one, that step is checked by reading.
