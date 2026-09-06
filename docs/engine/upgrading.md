<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Upgrading rust-mutants

**Status: implemented.** Each section says what changed for somebody who was
already running the release before it, and what to do about it. A change that
needs nothing is not listed.

## Upgrading to E5

**A run can now record what it did.** `--trace` takes an optional directory.
Without one, a run records under `<reports.directory>/<run id>/trace/` and
every other command under `<reports.directory>/traces/<run id>-<command>/`.
Both places are pruned by `reports.keep`, counted apart, so a recording never
costs a stored report its place. Nothing records unless `--trace` is given,
and a directory that cannot be created costs one line on standard error and
never the run.

**Reports carry three more fields per mutant and one more per refusal.** A
mutant row gained `start_byte`, `end_byte`, and `source_digest`; a refusal
gained `index`. Both schemas gained them in the same release, and the version
stayed 1: the rule is that an optional field added with its schema entry is
the same version. A reader that refused an unknown field will need the reader
from this release or later; the engine's own readers stopped refusing one.

**The findings enum gained `unreached-mutant`.** The engine has reported it
since coverage routing shipped and the published schema refused it, so a
consumer validating against `rust-mutants-run-report-v1.json` from an earlier
release would have rejected a valid coverage run. Take the schema from this
release.

**The exit code of a merged run changed.** `merge` derived the code from the
accounting columns and returned 2 for any run with mutants that were not run,
which made a coverage run whose only finding is `unreached-mutant` look like a
run that broke. It now derives the code from the findings, as an unmerged run
always did: a mutation nothing reaches is a gap in the tests and exits 1.

**An inconclusive mutant says which of the two things left it undecided.** The
detail said "timed out once and did not do so again" whether or not anything
had timed out. It now says that only after a retry, and otherwise says that no
test ran with the mutation active.

**`prune` no longer counts a directory that holds no run report.** A run
directory is one with a `run-report-v1.json` in it, and `reports.keep` bounds
those, the recordings of runs that wrote no report, and `traces/` as three
separate sets. Pruning also happens after a command that recorded, not only
after one that reported, so `--trace` on its own no longer grows the report
directory without limit. Anything else you kept under the report directory is
left alone.

**A coverage export whose region ends before it starts is refused.** Such a
region describes nothing, so `contains` answers no for every point in it and a
mutation a target really did reach would be routed away from that target. The
whole measurement is now refused instead, which routes every mutation
everywhere: what a measurement cannot say is what a run has to run. No export
`llvm-cov` writes contains one.

**`LLVM_PROFILE_FILE` is composed rather than inherited.** A run sets it to a
path under the temporary directory each execution owns, or to the coverage
pass's own path when it is the one measuring. If you were setting it around
`rust-mutants run` to measure a project's coverage, set it around the build
instead: an instrumented test process that inherited it wrote over the
measurement that started the run, and one with no path at all wrote
`default_*.profraw` into the tree being measured, which the drift report then
blamed on your tests. Running `rust-mutants` under `cargo llvm-cov` is
unaffected and is still allowed on the command line; only the three
`RUST_MUTANTS_*` variables are refused there.

**`RM5002` says what it observed.** It claimed the pristine tree passes, which
a run never establishes: it type-checks the tree rather than running it. It
now names the target that failed with nothing active and says that
`--no-verify` is the way past it.


## Upgrading to E6

**A run compiles what you tell it to.** The `[build]` section and its flags —
`--features`, `--all-features`, `--no-default-features`, `--build-target`,
`--profile`, `--build-jobs` — are passed to every command a run compiles with.
`--build-target` rather than `--target`, because `run --target` already names
a test target. The report's `selection.build` says what was used.

**Stored outcomes go cold once.** The cache key now covers the build
configuration, because the same tree compiled two ways is two programs and a
record kept for one of them answers nothing about the other. The first run
after this release re-executes everything; the ones after it reuse as before.

**A report says which test noticed a mutation.** A mutant row gained
`killed_by` and `signal`, and a `mutant-exec` recording gained the same two.
Both are optional and the schema version stayed 1.

**`build.rustflags` no longer stops the coverage measurement.** The flags a
cargo configuration file sets are read and put back into the instrumented
build, so a tree that sets one is measured rather than routed everywhere.
Flags set for a *target* (`[target.<triple>]`, `[target.cfg(…)]`) still refuse
the measurement: which of those tables apply is cargo's decision about the
target being built. The order the files are joined in was wrong and is now
cargo's own — the home directory first, the nearest file last.

**Exit code 98 from a test process is an error, not a kill.** It is the code a
probe process ends with when it cannot record what it saw, and reading it as a
failing test credited a test that never ran.

**A cancelled build is a cancellation.** `Ctrl-C` during a compilation used
to be read as a tree that does not compile: the half-finished command printed
nothing a diagnostic could be attributed to, so validation bisected and
condemned mutants nothing had refused. `cargo::CargoErrorKind::Cancelled` and
`ValidateError::Cancelled` both carry `RM0001`, and the run exits 130 having
written nothing.

**Attribution reads every span of a diagnostic, not only the primary one.**
The compiler points at the place it decided, which for a type error is often
the definition rather than the edit; the edit is named by another span of the
same message, or by one of its notes. Runs that used to bisect for such an
error now attribute it directly, which is faster and names the mutant with the
compiler's own words. Nothing that named no branch at all is attributed.

**A refusal says whether the compiler refused it on its own.** Rejection rows
gained `isolated`. Bisection compiles each offender it isolated once more,
alone, so the row carries the compiler's words about *that* mutant rather than
"no diagnostic named it"; a `bisect` recording says how many it could. When
neither half of a suspect set fails, the offence straddles them, and narrowing
by increasing granularity finds the mutants that interact rather than
condemning everything that was live. Those rows have `isolated: false` and say
which other mutants they were refused with.

**A snapshot directory that is already gone is removed.** Cleanup released the
lock, then retried five times with a backoff against a directory a sweeper had
already taken, and reported `RM1011` for it.
