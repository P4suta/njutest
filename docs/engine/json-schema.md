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

## Writers are strict, readers are lenient

A writer emits exactly what the schema describes, and a test proves it. A
reader ignores a field it does not know, so a document one release wrote is
read by the one before it.

That makes the version rule a short one. **Adding an optional field, and the
schema entry for it in the same change, keeps the version.** Making a field
required, renaming one, or changing what an existing one means is a new
version and a new schema file. A consumer that must know whether a field was
present reads it as absent rather than as a default; a consumer that reads a
document as a whole never fails on a field it has no use for.

The engine's own configuration file and its outcome store are the exception:
both refuse a key they do not know, because there a typo is a silent change
of what was asked rather than a field from the future.

## `rust-mutants/catalog` v1

```jsonc
{
  "document_type": "rust-mutants/catalog",
  "schema_version": 1,
  "tool_version": "0.1.0",
  "workspace": { "root_name": "…", "toolchain": "rustc 1.98.0 (…)", "workspace_digest": "<64 hex>",
                 "platform": { "os": "linux", "arch": "x86_64", "target": "x86_64-unknown-linux-gnu" } },
  "selection": { "profile": "balanced", "operators": [], "include": [], "exclude": [], "packages": [],
                 "build": [] },
  "mutants": [{
    "id": "<64 hex>", "display_id": "<20 hex>",
    "path": "crates/a/src/lib.rs", "package": "a",
    "family": "comparison", "rule": "le-to-lt", "rule_version": 1,
    "line": 12, "column": 9, "start_byte": 100, "end_byte": 102,
    "source_digest": "<64 hex>", "original": "<=", "replacement": "<",
    "branch": { "direction": "decreasing", "body_start": {"line": 12, "column": 14}, "body_end": {"line": 14, "column": 2} }
  }],
  "rejections": [{ "index": 7, "id": "…", "path": "…", "rule": "add-to-sub", "code": "E0369", "diagnostic": "…" }],
  "skips": [{ "path": "…", "reason": "macro-invocation", "count": 4 }]
}
```

`branch` is absent, not null, when no proof was claimed. `direction` is
diagnostic: a consumer must not branch on it.

`index` is dense over the accepted mutants and the refused candidates
together: every index from zero to their combined count appears exactly once
in one list or the other, which is what lets a reader check that a catalog
lost nothing. `path`, `rule`, `rule_version`, `start_byte`, `end_byte`,
`source_digest`, `original`, and `replacement` are exactly what minting the
identity takes, so a reader can re-mint `id` from the row and find out
whether it is the mutant it says it is.

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
  "selection": { "tier": "all", "operators": [], "include": [], "exclude": [], "packages": [],
                 "build": ["--features", "extra"] },
  "accounting": { "cataloged": 6, "refused": 0, "skipped": 5, "executed": 6,
                  "killed": 5, "survived": 1, "timed_out": 0, "inconclusive": 0,
                  "errored": 0, "not_run": 0, "unreached": 0, "expected": 0 },
  "score": { "detected": 5, "decided": 6, "value": 0.8333333333333334 },
  "mutants": [{ "…": "as in the catalog document, plus:",
                "outcome": "survived", "target": "a/lib/a", "exit_code": 0,
                "duration_ms": 41, "tests_run": 1, "killed_by": ["a::tests::bound"],
                "signal": null, "retried": false, "expected": false, "unreached": false }],
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

A `finding` is one of `surviving-mutant`, `inconclusive-mutant`,
`errored-mutant`, `not-run-mutant`, `unreached-mutant`, `discharged-mutant`,
`stale-expectation`, `unmatched-expectation`, or `unmatched-skip`. A mutant no
measured target reaches is an `unreached-mutant` finding rather than a
`not-run-mutant` one: it says the tests have a gap where the mutant is, not
that the run failed to get to it. A `discharged-mutant` says the same thing
about a mutation the tests do run and cannot observe: every target that could
have noticed it was removed by a proof. An `unmatched-skip` is a
`rust-mutants: skip` marker that hid nothing, which is a claim about code that
has moved or gone.

A mutant's `not_run_reason` says which of five things left it unexecuted:
`unreached` and `discharged` are proofs and are findings, `interrupted` is a
run that was killed, and `unselected` and `stopped-early` are the run doing
what it was asked to — a filter took the mutant out, or `--fail-fast` stopped
before reaching it. Neither of the last two is a finding, and both keep their
row, so a report of a narrowed run still accounts for the whole catalog it was
cut from.

## The run as it happens

`run --json` writes `rust-mutants-run-stream-v1`: one JSON object per line,
flushed as the run reaches it, for a program rather than a person. The kinds
are `run-start`, `phase-start`, `phase-end`, `mutant`, `finding`, `run-end`,
and `error`, each carrying a `type`. A reader takes a line at a time and
ignores a kind it does not know, which is what lets a later release say more
without breaking a consumer that already works;
`rust_mutants::report::stream::read_line` is that reader, shipped so a
consumer does not have to write one.

A `mutant` line is not a report row. It carries what a consumer needs the
moment a mutant is judged — what was mutated, where, and what the tests made
of it — and the report holds the rest, because a report is read afterwards
and a stream is read as it arrives.

`--json` and `--ui` are two ways of saying one thing, so a run is asked for
one or the other and never both.

## Evidence

A run that measured coverage writes `reached-v1.json` and `catalog-v1.json`
beside its report, and copies every probe log into `probe/`. They are the
premises its proof layers rest on: the measurement each target left behind,
the catalog with the branch bodies the compiler vouched for, and what each
probe process recorded. `cargo xtask engine-audit` reads them and re-decides
every route without the engine that produced them, which is what makes a
report's `discharged` a proof rather than a claim.

Writing them never fails a run. A file that could not be written is one an
audit calls unaudited, which is the honest answer.

## Recordings

`rust-mutants trace` writes JSON Lines rather than a document; its shape is
`schema/rust-mutants-trace-v1.json` and its rules are in
[trace](trace.md). A recording is never evidence, so nothing here reads one
to decide anything.

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
