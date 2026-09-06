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
directory is one with a `run-report-v1.json` in it. If you kept anything else
under the report directory, it is no longer removed by `reports.keep` and no
longer costs a run its place.
