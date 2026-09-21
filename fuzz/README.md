<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Fuzz targets

cargo-fuzz targets for the parts of this workspace that read input somebody
else wrote, or transform bytes. Each target states one property; a crash is
a bug in the property or the code, never in the input.

## Seeds

`fuzz/corpus/` is not committed, so a nightly run starts from nothing. For a
target whose input is a *document*, that means it starts and finishes without
once getting past the parser: random bytes are not JSON. Measured on
`offered_candidates` — fifty thousand runs from an empty corpus in under a
second and no coverage worth keeping, against three million runs in
forty-five seconds from three seed documents, with `candidates`, `version`
and `kind` in the dictionary it learned.

So `fuzz/seeds/<target>/` is committed and the workflow copies it into the
corpus before running. Three gates keep it honest: `xtask/tests/fuzz_ledger.rs`
refuses a seed directory that names no target or holds nothing, and
`crates/njutest-cli/tests/fuzz_seeds.rs` — with the three whose readers live
on the other side in `crates/rust-mutants-cli/tests/fuzz_seeds.rs` — puts
every seed to the reader its target uses. **A seed the reader refuses is not
a seed**: the target returns on the first line and the run explores what it
explored before. Writing these found two of mine that did exactly that.

A target whose input is bytes rather than a document needs none.

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
| `annotations` | never panics; every marker kept names a reason and a line; no marker means no annotated skip; deterministic |
| `config` | never panics; an accepted configuration has one canonical rendering and one 64-character digest |
| `coverage_export` | never panics; every accepted region ends where it began or after, and what a test reached is part of what the build instrumented |
| `libtest_lines` | never panics; every test name it reports is text that was there, and no line becomes two |
| `libtest_summary` | never panics; a target reaches `Passed` only through a line that counted a passing test, and a timeout is a failure whatever the line said |
| `report_document` | never panics; an accepted report round-trips, its record stream carries exactly one `VERDICT`, and one that fails the audit is refused by the write path |
| `model_result` | never panics; every Kani export is classified by the production strict parser as proved, noticed, or fail-closed undecided |
| `engine_config` | never panics; an accepted `.rust-mutants.toml` is one every later stage can honour, checked against the rules the reader states |
| `engine_coverage_export` | never panics; the engine's own reader accepts only regions `contains` can answer about, and what a run reached is part of what the build instrumented |
| `depinfo` | never panics; every unit source it accepts is a named Rust file, listed once |
| `cargo_config` | never panics; what it reads encodes back argument for argument, and a flag it accepts never holds the separator |
| `cargo_messages` | never panics; a diagnostic it accepts either names a whole primary span or names none |
| `cargo_metadata` | never panics; every package and target it accepts is named and rooted |
| `infection_log` | never panics; every index it accepts is one the catalog holds |
| `touch_log` | never panics; every index it accepts is one the catalog holds, and every record it attributes to a test is one that test and the target can both see |
| `duration` | never panics; what it renders it reads back as the same duration |
| `run_report` | never panics; an accepted run report renders, and one the reader refuses says why rather than panicking |
| `carried_answers` | never panics; a stream from another machine adds answers or is refused whole, and everything a machine holds is everything it hands on |
| `offered_candidates` | never panics; every candidate that comes back names a relative path, inside the tree, that the configuration allowed — because what comes back is what `fix --apply` writes |

```sh
mise run fuzz:smoke                     # every target, 2000 runs each
cargo +nightly fuzz run flatten         # one target, until interrupted
cargo +nightly fuzz run flatten -- -runs=100000
```

The crate is standalone (not a workspace member) because fuzzing needs
nightly and a sanitizer. Corpora and artifacts are ignored by git; a
reproducer worth keeping becomes a regular test.
