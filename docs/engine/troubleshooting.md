<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Troubleshooting

**Status: implemented.** Everything on this page is a command this release
has, an error code [`docs/errors.md`](../errors.md) documents, or a file a run
writes. Where a symptom has a remedy the engine itself knows, the engine says
it: an error carries the next step, and `doctor` carries one per check.

## By symptom

| What you see | What it is | What to do |
| --- | --- | --- |
| Every mutant is `inconclusive` | The tests never ran, or the target answers with a harness the engine cannot read | `rust-mutants doctor` for the `targets` check; a `harness = false` target answers by exit code alone, which is fine, but a target whose tests are all filtered out answers nothing |
| Every mutant is `errored` | The instrumented tree does not build for a reason that is not a mutant | `RM4001`/`RM5001`: make `cargo test --no-run` pass on the tree as committed |
| The run refuses before it starts, naming a variable | A test process would inherit an activation, so nothing it said would be about this run | unset the `RUST_MUTANTS_` variable; `doctor` and `diagnostics` report it rather than refusing, so you can still ask them |
| The run takes far longer than the test suite | Coverage routing measured nothing, so every mutant runs against every target | `doctor`'s `llvm-tools` check; `--dry-run` prints the estimate and the routing before anything runs |
| Survivors appear after an upgrade | New operators ask new questions | [upgrading](upgrading.md) names what each release added; `operators = [...]` pins the set |
| A run says `unmatched-skip` | A `rust-mutants: skip` marker or a `[[mutation.skip]]` entry hides nothing | remove it, or move it to the line it was meant for; a skip nobody can point at is one nobody can review |
| Two shards will not merge | `RM0011`: they are not parts of one catalog | merge the parts of one run — the same tree, the same catalog, one report per `--shard` |
| The temporary directory fills up | Snapshots and build caches an interrupted run left | `rust-mutants cache` says what is there, `cache --gc` removes what is abandoned, `--gc --all` the build caches too |
| `explain` shows no diff | The file is not the one the mutation was taken from | pass `--root` at the tree the run measured (`RM0012`), or re-run |

## By error code

Every code the engine can return is in [`docs/errors.md`](../errors.md) with
what it means and, where there is one, the remedy the engine prints with it.
The first two characters after `RM` say which layer answered:

| Range | Layer |
| --- | --- |
| `RM00xx` | the command line: configuration, stored reports, the environment |
| `RM10xx` | the workspace: the snapshot, the toolchain, cargo |
| `RM20xx` | discovery: what was compiled, what parses, what a marker says |
| `RM30xx` | instrumentation: guards, spans, the catalog |
| `RM40xx` | validation: what the compiler refused, and what could not be isolated |
| `RM50xx` | the session: the pristine build, verification, targets, mutants |
| `RM60xx` | coverage: the tools, the export, the profiles |
| `RM90xx` | values a caller gave: a rule name, a pattern, a duration |

## Asking `doctor`

```console
$ rust-mutants doctor
OK   cargo        cargo 1.98.0 (…)
OK   workspace    /home/you/project/Cargo.toml
WARN targets      demo-macros has no test target
          try: a package without a test target answers nothing; narrow with --package
OK   environment  no reserved variable is set
WARN snapshots    3 directories under /tmp, 0 kept on purpose
          try: `cache --gc` removes what is abandoned, `--gc --all` the build caches, `--gc --kept` what was kept
```

Each check stands `ok`, `warn`, or `fail`. **A warning is not a reason not to
run**: it says a run will cost more or measure less than it could. Only a
`fail` stops one, and the exit code follows — `0` when nothing failed, `2`
when something did. `--json` writes the same answer as
`rust-mutants/doctor` v1 for a program to read.

## Attaching a bundle

```console
$ rust-mutants diagnostics
reports/mutation/20260907T101122333Z/diagnostics
absent	probe
```

One directory holds everything a reader re-decides the run from: the run
report, the catalog, the measurement (`reached-v1.json`), the probe logs, the
recording, the configuration, a fresh `doctor-v1.json`, `toolchain.txt`, and
`environment.txt`. `bundle.json` names what is in it as `held` and what the
run did not leave as `absent`, so a reader can tell a run that had nothing to
say from a file that never arrived.

`environment.txt` carries the **names** of the variables that were set and no
value of any of them. Attach the directory as it is.

Pass a run id to bundle an older run
(`rust-mutants diagnostics 20260907T101122333Z`), and `--output DIR` to put
the bundle somewhere other than beside the run.

## Reading a trace

`run --trace` records what the run did into `<report directory>/<run
id>/trace/`. It is never evidence — nothing decides anything by reading one —
but it is how a run explains itself:

```console
$ rust-mutants trace summary          # what each phase cost, and the slowest commands
$ rust-mutants trace check            # whether the recording is whole
$ rust-mutants trace diff A B         # what moved between two runs
```

The vocabulary is in [trace](trace.md). A phase that took the time is the
first thing a summary shows; `route` records say why a mutant ran where it
did, and `select` records say why one did not run at all.
