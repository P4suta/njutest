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
