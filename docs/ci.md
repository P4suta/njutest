<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# CI usage

**Status: implemented.** Every job named here exists in `.github/workflows/`.

A repository will run mjutest from a tagged release or a checkout:

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
      - run: cargo install mjutest-cli --locked
      - name: Pull-request scope
        if: github.event_name == 'pull_request'
        run: mjutest verify --changed=origin/${{ github.base_ref }} --ui=plain
      - name: Full main scope
        if: github.event_name != 'pull_request'
        run: mjutest verify --ui=plain
      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: mjutest-reports
          path: reports/
```

To diagnose a run that only misbehaves on the runner, set `MJUTEST_TRACE: '1'`
on the verify step and upload `.mjutest/trace/` with the reports.

## The workflows of this repository

| Workflow | Jobs | When |
| --- | --- | --- |
| `ci.yml` | the three-OS test matrix, lint (fmt, clippy, rustdoc, the `cargo xtask` gates, typos, taplo, actionlint, committed), cargo-deny, cargo-audit, the coverage ratchet, and `ci-success` which gathers them | every push and pull request |
| `mutation.yml` | `cargo-mutants` over each package | weekly, and on request |
| `dogfood.yml` | `shard` runs the engine over its own catalog in four parts, and `audit` puts the parts back together, checks each recording, and re-decides every part against the ledger | weekly, and on request |
| `fuzz.yml` | every fuzz target for a fixed time | weekly, and on an engine pull request |
| `release-please.yml`, `release.yml` | the release train | on `main`, and on a tag |

The required checks are the ones `ci-success` gathers. `mutation.yml` and
`dogfood.yml` are the two independent measurements of how strong this suite
is, and neither gates a pull request: a survivor is a test to write or an
acceptance to record with a reason, which is work to schedule rather than a
push to block.

## Dogfooding the engine

`dogfood.yml` runs `rust-mutants` over its own engine with `--probe --jobs 4
--trace`, cut into four parts by `--shard K/4` so no part runs into the
120-minute limit and reports every remaining mutant as interrupted. Coverage
is measured because it is the default. The `audit` job then:

1. merges the four reports into the one the whole would have written,
2. asks `rust-mutants trace check` whether each recording is complete,
3. runs `cargo xtask engine-audit <part> --trace <part>/trace --ledger
   .rust-mutants.toml` over every part, which re-mints every identity,
   re-tallies every column, re-derives every discharge from the measurement
   and the catalog the part kept, holds every row to the recording of what
   actually ran, and refuses a survivor the ledger does not accept,
4. says what moved since the last complete run, with `xtask report-diff`
   against that run's own artifact. There is nothing to compare on the first
   run of the workflow, and a diff against nothing is not a failure; a diff
   that cannot be read is.

The first three steps run locally as `mise run dogfood:engine:audit`.

## Mutation testing somebody else's project

`rust-mutants` is a product in its own right, and this is what a project that
uses it puts in its own workflow. Nothing here is specific to this
repository.

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
      - uses: Swatinem/rust-cache@v2
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

**The gate is the merged exit code, not a percentage.** `merge` refuses parts
that are not parts of one catalog (`RM0011`), so a green `report` job means
every part measured the same tree with the same catalog and nothing was
silently lost. There is no threshold flag; see
[ADR 0004](adr/0004-proof-layers-not-budgets.md).

**Every part must be the same tree.** `workspace_digest` has to match across
shards for `merge` to accept them, so a Windows leg with `autocrlf` on cannot
be merged with a Linux one. Shard on one platform.

**The cache is per tree.** A run reads back what an earlier run of this exact
tree established, so a re-run of an unchanged commit is nearly free and a
changed commit is a cold cache. `--shard` is what makes a long run fit in a
job's limit; the cache is not.

**Narrow the pull-request leg.** `--changed` on a pull request measures only
what differs, which is usually the difference between two minutes and two
hours. Keep the whole catalog for the weekly run.
