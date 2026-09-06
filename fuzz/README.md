<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Fuzz targets

cargo-fuzz targets for the parts of this workspace that read input somebody
else wrote, or transform bytes. Each target states one property; a crash is
a bug in the property or the code, never in the input.

The runner's targets are here for a particular reason: three of the four
read something a *test suite* can influence. A suite that printed its own
`test result:` line, an `llvm-cov` export from a version nobody expected, a
report document edited by hand — each of those decides what a run claims,
so each has to fail closed rather than plausibly.

| Target | Property |
| --- | --- |
| `flatten` | never panics; an accepted result has no line break |
| `trace_reader` | never panics; accepted events round-trip through the writer's encoding |
| `glob` | never panics; a literal pattern matches its own spelling |
| `splice` | never panics; an accepted set yields a monotone offset map of the right length |
| `normalize_path` | never panics; a normalized path is a fixed point |
| `discover_file` | never panics; every candidate validates, is spanned from the source, sits inside its site; deterministic |
| `config` | never panics; an accepted configuration has one canonical rendering and one 64-character digest |
| `coverage_export` | never panics; every accepted region ends where it began or after, and what a test reached is part of what the build instrumented |
| `libtest_summary` | never panics; a target reaches `Passed` only through a line that counted a passing test, and a timeout is a failure whatever the line said |
| `report_document` | never panics; an accepted report round-trips, its record stream carries exactly one `VERDICT`, and one that fails the audit is refused by the write path |
| `engine_config` | never panics; an accepted `.rust-mutants.toml` is one every later stage can honour, checked against the rules the reader states |
| `engine_coverage_export` | never panics; the engine's own reader accepts only regions `contains` can answer about, and what a run reached is part of what the build instrumented |
| `depinfo` | never panics; every unit source it accepts is a named Rust file, listed once |
| `cargo_messages` | never panics; a diagnostic it accepts either names a whole primary span or names none |
| `cargo_metadata` | never panics; every package and target it accepts is named and rooted |
| `infection_log` | never panics; every index it accepts is one the catalog holds |
| `duration` | never panics; what it renders it reads back as the same duration |
| `run_report` | never panics; an accepted run report renders, and one the reader refuses says why rather than panicking |

```sh
mise run fuzz:smoke                     # every target, 2000 runs each
cargo +nightly fuzz run flatten         # one target, until interrupted
cargo +nightly fuzz run flatten -- -runs=100000
```

The crate is standalone (not a workspace member) because fuzzing needs
nightly and a sanitizer. Corpora and artifacts are ignored by git; a
reproducer worth keeping becomes a regular test.
