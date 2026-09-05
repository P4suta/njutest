<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# JSON documents of the engine

**Status: implemented and validated. The catalog document is
`schema/rust-mutants-catalog-v1.json`, the run report
`schema/rust-mutants-run-report-v1.json`.**
Both carry `document_type` and `schema_version`, close every object with
`additionalProperties: false`, and are validated by tests against the
schemas under `schema/`, so a field added without a version bump fails a
test rather than a consumer.

## `rust-mutants/catalog` v1

```jsonc
{
  "document_type": "rust-mutants/catalog",
  "schema_version": 1,
  "tool_version": "0.1.0",
  "workspace": { "root_name": "…", "toolchain": "rustc 1.98.0 (…)", "workspace_digest": "<64 hex>",
                 "platform": { "os": "linux", "arch": "x86_64", "target": "x86_64-unknown-linux-gnu" } },
  "selection": { "profile": "balanced", "operators": [], "include": [], "exclude": [], "packages": [] },
  "mutants": [{
    "id": "<64 hex>", "display_id": "<20 hex>",
    "path": "crates/a/src/lib.rs", "package": "a",
    "family": "comparison", "rule": "le-to-lt", "rule_version": 1,
    "line": 12, "column": 9, "start_byte": 100, "end_byte": 102,
    "original": "<=", "replacement": "<",
    "branch": { "direction": "decreasing", "body_start": {"line": 12, "column": 14}, "body_end": {"line": 14, "column": 2} }
  }],
  "rejections": [{ "id": "…", "path": "…", "line": 3, "column": 5, "rule": "add-to-sub", "diagnostic": "…" }],
  "skips": [{ "path": "…", "reason": "macro-invocation", "count": 4 }]
}
```

`branch` is absent, not null, when no proof was claimed. `direction` is
diagnostic: a consumer must not branch on it.

## `rust-mutants/run-report` v1

Written by `rust-mutants run` to
`<reports.directory>/<run id>/run-report-v1.json`, with
`<reports.directory>/latest.json` naming the newest.

```jsonc
{
  "document_type": "rust-mutants/run-report",
  "schema_version": 1,
  "tool_version": "0.1.0",
  "run": { "id": "20260905T132650666Z", "started_at": "…", "finished_at": "…",
           "duration_ms": 812, "interrupted": false, "exit_code": 1 },
  "workspace": { "…": "as in the catalog document, plus catalog_digest" },
  "selection": { "tier": "all", "operators": [], "include": [], "exclude": [], "packages": [] },
  "accounting": { "cataloged": 6, "refused": 0, "skipped": 5, "executed": 6,
                  "killed": 5, "survived": 1, "timed_out": 0, "inconclusive": 0,
                  "errored": 0, "not_run": 0, "unreached": 0, "expected": 0 },
  "score": { "detected": 5, "decided": 6, "value": 0.8333333333333334 },
  "mutants": [{ "…": "as in the catalog document, plus:",
                "outcome": "survived", "target": "a/lib/a", "exit_code": 0,
                "duration_ms": 41, "tests_run": 1, "retried": false, "expected": false,
                "unreached": false }],
  "rejections": [], "skips": [],
  "expectations": [{ "id": "…", "reason": "…", "outcome": "survived", "mutant": "<64 hex>",
                     "standing": "met", "actual": null, "why": null }],
  "findings": [{ "kind": "surviving-mutant", "mutant": "<64 hex>", "detail": "…" }]
}
```

The outcome columns add up: `killed + survived + timed_out + inconclusive +
errored == executed`, and `executed + not_run == cataloged`. `unreached`
counts the `not_run` mutants a coverage measurement proved no target reaches,
so it is never larger than `not_run` and is zero in a run that measured none. `score` is
`detected / decided` where `detected = killed + timed_out` and `decided =
detected + survived`; it is **absent** when the run decided nothing, which is
not the same as a score of zero. A timeout is `timed_out` only after a serial
retry timed out again; one that did not reproduce is `inconclusive`, which is
a hole rather than a detection.

`exit_code` is the one the process returned: `0` every mutant was noticed,
`1` something was not, `2` the run itself failed, `130` it was interrupted.

## Infection log

```text
rust-mutants-infection-v1 <catalog digest> <N>
<index>
<index>
```

Written by the probe runtime with `O_APPEND`, one header per process, one
index per infected mutant, first time only. Read fail-closed: a truncated
line, a mismatched header, or an index beyond the catalog yields no facts at
all — never the parseable prefix, which is exactly what a smaller wrong answer
looks like.
