<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Configuration v1

**Status: implemented** (`mjutest_cli::config`). The defaults, the
strictness, and the two rules about what a report may contain are fixed by
tests; `mjutest init` writes the skeleton below, and a test loads the
untouched skeleton and asserts it is exactly the defaults.

`.mjutest.toml` is optional and strict. Missing configuration uses
`standard-v1`, the whole workspace, a ten-minute execution timeout, and a
cache capped at 5 GiB and 30 days. Unknown keys, malformed values, and any
`version` other than `1` are errors.

`mjutest init` writes an annotated skeleton: the two active defaults, and
every section below as commented guidance. Loading the untouched skeleton
yields exactly the defaults.

```toml
version = 1
contract = "standard-v1"        # "standard-v1" | "deep-v1"

[project]
packages = []                   # cargo package names; empty = every workspace member
exclude = ["**/generated/**"]   # workspace-relative globs; an explicit limitation in the report

[execution]
features = []                   # cargo features
all_features = false
no_default_features = false
test_binary_args = []           # allowed: --test-threads=N, --include-ignored, --nocapture, --show-output
environment = []                # variable names only, never values; RUST_TEST_* is refused
timeout = "10m"                 # upper bound for one executed command; Go duration syntax
jobs = 0                        # mutation workers; 0 = logical CPUs capped at four

[cache]
max_bytes = 5368709120          # 5 GiB
ttl = "720h"                    # 30 days
build_max_bytes = 8589934592    # 8 GiB, the machine-wide build cache
build_dir = ""                  # default: below the user cache directory

[reports]
keep = 20                       # run directories kept under reports/runs

[fuzz]
run = false                    # drive the fuzz targets, not only find them
max_total_time = "60s"         # per target
targets = []                   # empty = every target the tree holds

[soundness]                     # deep-v1 only
miri_flags = []
sanitizers = []                 # e.g. ["thread"] on nightly

[resources.postgres]
command = ["./tools/postgres-provider"]
timeout = "30s"
shared = true                   # or exclusive = true (forces jobs = 1)
environment = ["POSTGRES_IMAGE"]

[generation]
command = ["./tools/test-generator"]
allowed_paths = ["**/tests/**/*.rs", "**/fuzz/corpus/**"]
environment = ["GENERATOR_TOKEN"]

[[acceptance]]
id = "0123456789abcdef"
reason = "reviewed equivalent boundary"
expires = "2026-12-31T00:00:00Z"
owner = "quality-team"
ticket = "QA-123"
```

There is no `profile` key — `cargo test`'s `test` profile is the one under
verification — and no `toolchain` key: `rust-toolchain.toml` is the idiomatic
pin and `rustc -vV` is recorded. Environment entries are names, never
`KEY=value`; values are not written to reports. `mjutest accept` appends an
`[[acceptance]]` table while preserving the comments of the file.

mjutest owns the libtest flags that alter routing, repetition, selection,
output protocol, or completeness — positional filters, `--exact`, `--list`,
`--ignored`, `--skip`, `--format`, `--logfile`, `--test`, `--bench`, `-q`,
`--color`, `--report-time`, `--shuffle*`, `-Z*` — and rejects them after
`--`.
