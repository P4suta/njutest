<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Provider protocols v1

**Status: the resource protocol is implemented; generation is contract only.** Ported unchanged from goatest;
both protocols use newline-delimited strict JSON and reject unknown fields.
They are local subprocess contracts; core performs no network calls.

## Resource provider

One long-lived process reads a start request:

```json
{"version":1,"action":"start","capability":"postgres","request_id":"resource-000001"}
```

It replies on one line:

```json
{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}
```

On lease release mjutest sends a `stop` action with the same fields plus
`instance`; the final response must be `{"version":1,"status":"stopped",…}`
and the process must exit. Startup and shutdown are timeout- and
process-tree-bounded. Returned environment cannot override toolchain,
temporary-directory, `MJUTEST_*`, `RUST_MUTANTS_*`, or `CARGO_*` variables.

`shared=true` reuses one live instance while leases exist. `exclusive=true`
serializes the capability and constrains mutation jobs.

## Generation provider

Generation is one process per finding. stdin receives the finding (id, kind,
path, line, summary, replay command, mutant description and id), the allowed
paths, and the snapshot identity; stdout returns one strict object with up to
64 candidates of kind `patch` or `corpus`, each with a path, the SHA-256 of
the preimage, and base64 content. At most 4 MiB of provider output is
accepted. Allowed paths are independently confined to test files and fuzz
corpora; a patch to a `#[cfg(test)] mod tests` inside `src/` is not accepted
in v1.

Provider output never changes the worktree. Each candidate must pass three
original-code stability runs, two mutant-kill checks, the related suite
validation, and preimage checks before it is stored. Explicit `fix --apply`
repeats fresh validation and preimage checks.
