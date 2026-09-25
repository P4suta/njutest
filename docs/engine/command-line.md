<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The command line

**Status: implemented.** Every flag on this page exists, and a test compares this page with `--help` in both directions: a flag the binary has and this page does not name fails, and so does a flag this page names and no command has.

Every value here also has a key in [configuration](configuration.md), and a flag given on the command line wins over the file.

## Everywhere

| Flag | What it does |
| --- | --- |
| `--help`, `--version` | the usage, the release |
| `--root DIR` | the workspace root; the working directory when absent |
| `--config FILE`, `--no-config` | read the configuration elsewhere, or not at all |
| `--color auto\|always\|never` | when to colour; `NO_COLOR` and a pipe both mean never |
| `--trace[=DIR]` | record what the command did, as JSON Lines |

`rules` is the exception: the operator table is compiled into the release, so it reads no tree and takes none of these but `--help`.

## Choosing what to measure

These narrow a run, and `list`, `catalog`, `why-skipped` and `instrument` read them too, so what those preview is what a run would do.

| Flag | What it does |
| --- | --- |
| `--package NAME` | only these packages; repeatable |
| `--include GLOB`, `--exclude GLOB` | workspace-relative globs naming what is and is not mutated; repeatable |
| `--omit GLOB` | workspace-relative globs the copy does not carry at all; repeatable |
| `--changed`, `--changed-from REV` | only the files that differ from `HEAD` or from a revision; a change that touches no Rust file the configuration measures prints `NOTHING`, names what did change, and exits 0 without opening the workspace |
| `--tier balanced\|strong\|all` | which operators the run asks |
| `--operator NAME` | exactly these rules, whatever the tier says |
| `--rule NAME`, `--family NAME` | keep only these; repeatable |
| `--skip-rule NAME`, `--skip-family NAME` | drop these; repeatable |
| `--file PATH[:LINE[-LINE]]` | keep only this file, or these lines of it |
| `--id PREFIX` | keep only the mutants whose identity starts this way |
| `--from-report [RUN]`, `--outcome OUTCOME` | keep only what an earlier run left in that state |
| `--shard K/N` | one part of the catalog, cut by index |

## Building and running

| Flag | What it does |
| --- | --- |
| `--features LIST`, `--all-features`, `--no-default-features` | what cargo compiles |
| `--build-target TRIPLE`, `--profile NAME`, `--build-jobs N` | how cargo compiles it |
| `--allow-outside DIR` | let the build read a directory outside the root, copied into the snapshot where the tree reaches it |
| `--offline`, `--locked` | what cargo may reach for and change |
| `--jobs N\|auto\|all`, `-j` | mutants measured at once: a count, `auto` (the machine, capped at 4; the default off CI), or `all` (every processor, for a runner doing nothing else; the default under CI); `0` is refused, since `auto` says it |
| `--timeout DURATION` | a mutant's own bound; five times the target's baseline when absent |
| `--no-verify` | do not run the instrumented baseline first |
| `--no-doctests` | leave a library's documented examples out |
| `--skip-target ID` | never start this target, as `pkg/kind/name`; a name the workspace declares no target for is refused (`RM5004`) |
| `--test NAME`, `--target NAME` | one test, one test target |
| `--mutant PREFIX` | one mutant |
| `--coverage`, `--no-coverage` | build once with LLVM coverage instrumentation, and route by what it measured |
| `--no-touch` | do not ask the guards which of each target's tests reached them, and so run every test of every target that could |
| `--equivalence` | ask the compiler whether a survivor's mutation is one it renders identically |
| `--no-cache` | remeasure coverage, baseline and mutant outcomes instead of reading back an exact prior answer |
| `--keep-temp` | keep the snapshot and its build cache, and record where |
| `--fail-fast` | stop at the first finding |
| `--dry-run` | prepare, verify, and say what a run would cost |
| `--run-id NAME` | name the run, and its report directory |
| `--no-report` | establish it and store nothing |
| `--ui auto\|plain\|quiet` | what a run says while it happens |
| `--json` | one object per line, for a program |

## Reading it back

| Command | Flags |
| --- | --- |
| `report` | `--run ID`, `--format lines\|json\|markdown\|junit\|sarif\|html\|stryker`, `--output FILE`, `--tui` |
| `explain PREFIX` | `--run RUN`, `--fresh`, `--json` |
| `replay PREFIX` | `--run RUN` |
| `why-skipped` | `--file PATH`, `--line N` |
| `catalog` | `--rejections`, `--json` |
| `instrument` | `--mutant PREFIX` |
| `equivalence` | `--limit N` |
| `trace` | `summary`, `check`, `diff` |
| `merge` | `--runs IDS`, `--output FILE` |
| `diagnostics [RUN]` | `--output DIR` |
| `rules` | `--tier TIER`, `--json` |
| `doctor` | `--json` |
| `init` | `--force` |
| `cache` | `--gc`, `--all`, `--kept`, `--clear-outcomes`, `--cache-dir DIR`, `--export FILE` (the outcome store as one document, each record carrying what it was keyed on), `--import FILE` (file an exported store under the keys its records derive, refusing another release's) |
| `ci gate` | `--run ID`, `--report FILE`, `--sarif FILE`, `--host github\|gitlab\|plain`, `--changed-from REV` |

`--run` names a stored run, and the newest is read when nothing is named.
A name no directory answers to is refused (`RM0007`) rather than answered from another run: a reader who mistyped it would otherwise be told confidently about a run they did not ask for, and `replay` would report the stored answer as having changed when what changed was which run it read.

## In a continuous integration job

`ci gate` reads one run, a stored one or the report `--report` names, writes it where the job shows it, and exits with the run's own code.
The host is the one the environment names, or the one `--host` asks for; asking for GitHub outside a GitHub Actions step is refused (`RM0014`).

On GitHub Actions it appends the Markdown report to the step summary, appends `verdict=`, `report=` and, with `--sarif`, `sarif=` to the step outputs, and writes one `::error` annotation per survivor no claim accounts for.
`verdict` is `detected`, `found`, `failed` or `interrupted`, the words for exit codes 0, 1, 2 and 130.
A runner shows ten error annotations per step; past that the summary says how many were shown of how many, and where all of them are.
An annotation's file is named from the checkout `GITHUB_WORKSPACE` names, so a root outside it is refused (`RM0015`) rather than annotated where the runner cannot place it.
With `--changed-from REV`, only the survivors on lines that differ from that revision, committed and not, are annotated, and the summary says how many others there are; a revision git cannot answer about is refused (`RM0010`).
Every line of a file git does not track counts as changed.
On GitLab CI, and with `--host plain`, it writes the lines `report` writes.

## What a run's exit code says

| Code | What it means |
| --- | --- |
| 0 | every mutant the run decided, the tests noticed |
| 1 | there is a finding about the tests: a survivor, a mutation no test reached or a proof removed, a mutation the run could not decide either way, or a stale or unmatched claim |
| 2 | the run could not measure a mutation it ran — it waited, reached its step limit, errored or was not run — or the run itself failed, or the command was used wrongly |
| 130 | it was interrupted |
| 143 | it was terminated, which is what a cancelled job sends |

The table is `rust_mutants::run::Exit`, which is also what a run decides its code with and what `--help` prints, so the three cannot say different things; a test holds this page to it.

An exit code is about what was established, never about a percentage.
There is no threshold flag; see [ADR 0004](../adr/0004-proof-layers-not-budgets.md).
