<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# CI usage

**Status: contract only.** The binaries do not verify anything yet.

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

For this repository itself, the required checks are in `.github/workflows/ci.yml`:
the three-OS test matrix, lint (fmt, clippy, rustdoc, the `cargo xtask` gates,
typos, taplo, actionlint, committed), cargo-deny, cargo-audit, and the coverage
ratchet, all gathered by `ci-success`. `mutation.yml` runs cargo-mutants
weekly.
