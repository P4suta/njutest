<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# JSON documents of the engine

**Status: the catalog document is implemented and validated
(`schema/rust-mutants-catalog-v1.json`); the run report arrives in E2.**
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
