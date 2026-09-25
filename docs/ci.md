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
        run: njutest verify --changed=origin/${{ github.base_ref }} --ui=plain
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
| `ci.yml` | the three-OS test matrix, lint (fmt, clippy, rustdoc, the `cargo xtask` gates, typos, taplo, actionlint, committed), cargo-deny, cargo-audit, the coverage ratchet, the `book` build, `soundness`, `action-smoke`, and `ci-success` which gathers them | every push and pull request |
| `mutation.yml` | `cargo-mutants` over each package | weekly, and on request |
| `dogfood.yml` | `shard` runs the engine over its own catalog in four parts, and `audit` puts the parts back together, checks each recording, and re-decides every part against the ledger | weekly, and on request |
| `fuzz.yml` | every fuzz target for a fixed time | weekly, and on an engine pull request |
| `dependabot-auto-merge.yml` | asks for the merge of a dependency bump, which GitHub performs once `ci-success` passes; the label `no-auto-merge` says not to | on a dependabot pull request |
| `release-plz.yml`, `release.yml` | the release train | every push to `main`, and on a tag |
| `codeql.yml` | CodeQL over Rust and over the workflows, with the `security-extended` queries; `CodeQL required` is the one name a protection rule asks for | every push and pull request, and weekly |
| `dependency-review.yml` | what a pull request adds to the dependency graph, refused at moderate severity in any scope | on a pull request |
| `scorecard.yml` | the OpenSSF Scorecard of this repository, published | every push to `main`, and weekly |

The `book` job builds `docs/` with mdbook, which refuses a summary that names a page the repository does not hold; `cargo test -p xtask --test docs` refuses the other direction, a page the summary does not name.
A page that neither side notices is one a reader of the book cannot reach.

`codeql.yml`, `dependency-review.yml`, and `scorecard.yml` answer about the supply chain rather than about this code: what a query finds in it, what a change adds to the graph below it, and what the posture of the repository looks like from outside.
`cargo deny` and `cargo audit` in `ci.yml` ask the same question of the graph that is already here, on every push; dependency review asks it of the difference, and says so on the pull request.
Secret scanning, push protection, Dependabot security updates, and private vulnerability reporting are repository settings rather than workflows, and are on.

Every workflow declares `shell: bash` as its default, which GitHub runs as `bash -e -o pipefail` on each of the three platforms, and no step or composite action names another shell (`cargo test -p xtask --test workflows`).
Without that default a Linux or macOS step runs in `bash -e`, where `njutest verify | tee out` has the status of `tee`, and a Windows step runs in PowerShell, which goes on past a native command that failed.
The first let the `soundness` job read on after an exit code nobody had looked at; the second once reported a failing test as a cancelled job forty-five minutes later.

The required checks are the ones `ci-success` gathers.
`mutation.yml` and `dogfood.yml` are the two independent measurements of how strong this suite is, and neither gates a pull request: a survivor is a test to write or an acceptance to record with a reason, which is work to schedule rather than a push to block.

## Dogfooding the engine

`dogfood.yml` runs `rust-mutants` over its own engine with `--probe --jobs 4 --trace`, cut into four parts by `--shard K/4` so no part runs into the 120-minute limit and reports every remaining mutant as interrupted.
Coverage is measured because it is the default.
The `audit` job then:

1. merges the four reports into the one the whole would have written,
2. asks `rust-mutants trace check` whether each recording is complete,
3. runs `cargo xtask engine-audit <part> --trace <part>/trace --ledger .rust-mutants.toml` over every part, which re-mints every identity,
   re-tallies every column, re-derives every discharge from the measurement and the catalog the part kept, holds every row to the recording of what actually ran, and refuses a survivor the ledger does not accept,
4. says what moved since the last complete run, with `xtask report-diff` against that run's own artifact.
   There is nothing to compare on the first run of the workflow, and a diff against nothing is not a failure; a diff that cannot be read is.

The first three steps run locally as `mise run dogfood:engine:audit`.

## Mutation testing somebody else's project

`rust-mutants` is a product in its own right, and this is what a project that uses it puts in its own workflow.
Nothing here is specific to this repository.

```yaml
name: mutation
on:
  pull_request:
  schedule: [{ cron: "17 5 * * 2" }]

jobs:
  measure:
    runs-on: ubuntu-latest
    timeout-minutes: 120
    strategy:
      fail-fast: false
      matrix:
        shard: [1, 2, 3, 4]
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0 # --changed needs history to see what changed
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools # coverage routing; without it every mutant runs everywhere
      - uses: Swatinem/rust-cache@v1
      - run: cargo install rust-mutants-cli --locked
      - run: rust-mutants doctor
      - name: Measure one part of the catalog
        run: |
          rust-mutants run --locked --jobs 4 \
            --shard ${{ matrix.shard }}/4 \
            --run-id "${{ github.run_id }}-${{ matrix.shard }}of4" \
            --json > "mutants-${{ matrix.shard }}.jsonl"
        continue-on-error: true # the merged report is the gate, not one part
      - uses: actions/upload-artifact@v4
        with:
          name: mutants-${{ matrix.shard }}
          path: reports/mutation/

  report:
    needs: measure
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install rust-mutants-cli --locked
      - uses: actions/download-artifact@v4
        with:
          path: reports/mutation/
          merge-multiple: true
      - name: Put the parts back together
        run: rust-mutants merge --runs "${{ github.run_id }}-*" --output mutants.json
      - run: rust-mutants report --format markdown >> "$GITHUB_STEP_SUMMARY"
      - run: rust-mutants report --format junit --output mutants.xml
      - run: rust-mutants report --format sarif --output mutants.sarif
      - uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: mutants.sarif
      - uses: actions/upload-artifact@v4
        with:
          name: mutation-report
          path: |
            mutants.json
            mutants.xml
```

Four things are worth saying about that file.

**The gate is the merged exit code, not a percentage.** `merge` refuses parts that are not parts of one catalog (`RM0011`), so a green `report` job means every part measured the same tree with the same catalog and nothing was silently lost.
There is no threshold flag; see [ADR 0004](adr/0004-proof-layers-not-budgets.md).

**Every part must be the same tree.** `workspace_digest` has to match across shards for `merge` to accept them, so a Windows leg with `autocrlf` on cannot be merged with a Linux one.
Shard on one platform.

**The cache is per tree.** A run reads back what an earlier run of this exact tree established, so a re-run of an unchanged commit is nearly free and a changed commit is a cold cache.
`--shard` is what makes a long run fit in a job's limit; the cache is not.

**Narrow the pull-request leg.** `--changed` on a pull request measures only what differs, which is usually the difference between two minutes and two hours.
Keep the whole catalog for the weekly run.

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
- run: njutest cache --import answers.jsonl || true
- run: njutest verify --locked
  continue-on-error: true
- run: njutest cache --export answers.jsonl
```

The import is allowed to fail on the first run of a repository, when there is no file yet; nothing else here is.
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
The action's outputs are `verdict` and `report`.

The step ends with the verdict's own exit code, so a `DEFECT` fails the job —
**after** the findings have been uploaded.
A run that swallowed the status to upload first, or uploaded nothing because the status was non-zero, would leave a person reading a log instead of the findings themselves.

`cargo binstall njutest` is what installs it, from the archive `release.yml` publishes, so the action does not compile this workspace inside somebody else's job.
`cargo njutest verify` works too, wherever the binary is on the path: the same program answers to the name cargo looks for.

A workflow that has already put `njutest` on the path keeps it: the action installs nothing unless `version` names a release or the binary is absent.
A job that built the commit under test, restored a cached binary, or installed from somewhere else has said which one it wants, and installing over it would answer a question nobody asked.

That is also what makes the action testable here.
`action-smoke` builds this workspace's own `njutest`, puts it on the path, and runs the action against `fixtures/fixture-assured` the way another repository would, checking that the `verdict` output is the one the run reached and that `report` names a file that exists.
What it does not exercise is the install itself, which needs a published release; until there is one, that step is checked by reading.
