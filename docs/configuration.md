<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Configuration v1

**Status: implemented** (`njutest_cli::config`). The defaults, the
strictness, and the two rules about what a report may contain are fixed by
tests; `njutest init` writes the skeleton below, and a test loads the
untouched skeleton and asserts it is exactly the defaults.

`.njutest.toml` is optional and strict. Missing configuration uses
`standard-v1`, the whole workspace, a ten-minute execution timeout, and a
cache capped at 5 GiB and 30 days. Unknown keys, malformed values, and any
`version` other than `1` are errors.

`njutest init` writes an annotated skeleton: the two active defaults, and
every section below as commented guidance. Loading the untouched skeleton
yields exactly the defaults.

```toml
version = 1
contract = "standard-v1"        # "standard-v1" | "deep-v1"

[project]
packages = []                   # cargo package names; empty = every workspace member
exclude = ["**/generated/**"]   # workspace-relative globs; the files are not mutated

[execution]
features = []                   # cargo features
all_features = false
no_default_features = false
test_binary_args = []           # allowed: --test-threads=N, --include-ignored, --nocapture, --show-output
environment = []                # variable names only, never values; RUST_TEST_* is refused
timeout = "10m"                 # upper bound for one executed command; Go duration syntax
jobs = 0                        # mutation workers; 0 = logical CPUs capped at four
skip_targets = []               # stable target ids never to start; every one is reported

[mutation]
equivalence = false             # ask the compiler whether it renders each survivor identically

[cache]
max_bytes = 5368709120          # 5 GiB
ttl = "720h"                    # 30 days

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

`build_max_bytes` and `build_dir` were removed from `[cache]`. Remove those
keys when upgrading: configuration is strict, so keeping either one is an
unknown-field error. Compiled artifacts belong to rust-mutants' stable target
directory; `[cache]` now controls only the outcome store described here.

`[project] packages` is what a run is about, and `--package` on the command
line takes its place rather than adding to it. A package no member answers to
is refused, the way `njutest plan` has always refused one: a run narrowed to a
name nobody wrote would measure the whole workspace and report the narrow
verdict `SCOPE_ASSURED` over it.

`[execution] test_binary_args` is how this project's suite runs, and every
test process of a run is started with those arguments, the baseline included:
a baseline taken one way and mutations measured another compares two suites.
Arguments given after `--` take the place of the ones the file holds rather
than adding to them.

`[execution] skip_targets` is the narrow escape hatch for a process whose tests
inspect the instrumented tree itself, or otherwise fail for the same known
reason under every mutation. Each entry is the stable target id a report and
`njutest plan` name, such as `pkg/test/ui`. An id the workspace does not declare
is an error, and every id that is left out is recorded as
`target-skipped-by-configuration`; it is never a silent pass.

`[execution] features`, `all_features` and `no_default_features` are the words
cargo would have been given, and every command of a run is given them: the
baseline, the mutation phase, the equivalence layer, `plan`, `replay` and
`fix`. Cargo compiles a different program for a different feature set, so a
run that measured the default build while the project ships another would put
a verdict on a program nobody runs.

`[project] exclude` says which files are mutated and nothing else. Every file
it names is still copied into the tree, still compiled, and still run, so the
patterns never turn a workspace that builds into one that does not, and a run
is a function of their bytes whether or not a mutation was put to them: the
evidence a run leaves behind is keyed on the whole tree, and `njutest watch`
starts a round when one of them changes. What the exclusion removes is
findings, which is why the report carries the patterns in `scope.excluded` and
why a run left with no mutation to put to a test concludes `INSUFFICIENT`
rather than assuring what it did not ask. A pattern that is not a pattern —
a leading or trailing `/`, an empty string — is refused when the file is read,
so a typo narrows nothing silently. The engine spells the same key
differently: `rust-mutants`' `[project] exclude` removes a path from the
snapshot as well, which is a decision a tool that only mutates can take and an
assurance runner cannot.

There is no `profile` key — `cargo test`'s `test` profile is the one under
verification — and no `toolchain` key: `rust-toolchain.toml` is the idiomatic
pin and `rustc -vV` is recorded. Environment entries are names, never
`KEY=value`; values are not written to reports. `njutest accept` appends an
`[[acceptance]]` table while preserving the comments of the file, and writes
every field one can carry. An acceptance whose `expires` has passed answers
for nothing and the findings it was hiding are raised again; one that names no
date never lapses.

njutest owns the libtest flags that alter routing, repetition, selection,
output protocol, or completeness — positional filters, `--exact`, `--list`,
`--ignored`, `--skip`, `--format`, `--logfile`, `--test`, `--bench`, `-q`,
`--color`, `--report-time`, `--shuffle*`, `-Z*` — and rejects them after
`--`.
