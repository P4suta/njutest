<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0043 — A test may decline to measure

## Status

Accepted, 2026-09-26.
Implemented in the engine by `decline::{Declines, held, storable}`, `MutantConclusion::{Declined, DeclinedUnderTheMutant}`, `NotRunReason::Declined`, the run report's `declined` rows and count, the `declined` lists of the `verify` and `mutant-exec` trace records, and engine-audit's recording layer; njutest reads a declined mutation as a gap.

## Context

A test that cannot establish anything on the machine it runs on often returns early and passes: storage-scout's sharing tests print `skipping: cannot create a volume that shares blocks here` on ext4 and return.
libtest counts that as a pass, and so does this engine, so a mutation only those tests reach survives: the run reports a gap in the tests that is really a machine the tests could not use.
[ADR 0042](0042-a-claim-holds-where-its-facts-do.md) lets a claim say where it holds, but a survivor that is not one needs no claim at all; it needs the run to know it was not measured.

libtest has no dynamic skip, and a pass is the only thing a returning test can say to it.
So the test has to say it to this engine instead, on a channel the engine opens for each process it starts, the way a crash notice already reaches it (`RUST_MUTANTS_CRASH_NOTICE`).

## Decision

**The engine names a file, and a test that declines writes one line to it.** Every test process the engine starts gets `RUST_MUTANTS_DECLINE_NOTICE`, the path of a file in that process's own scratch directory.
A test that cannot measure here appends one line, `<its libtest name>\t<why>\n`, in one write, and returns.
The decline is the test's last act: it sets the whole test aside, so a test that measured part of its work and declines the rest would hide what the measured part let survive, and a test that skips a section and goes on measuring does not decline.
The name is the one libtest gives the test's thread, which a helper reads rather than a person types.
libtest runs tests on several threads, and a line appended in pieces can have another test's land inside it; one write to a file opened for appending cannot.
Nothing is needed from this engine to do it: the protocol is an environment variable and a line of text, so a test suite adopts it with the file API it already uses, and a suite that never runs under the engine never sees the variable.

**A line is attributed only where the process's own reading is whole.** The engine names a test from libtest's `test … ok` lines, and it believes that list only when those lines add up to libtest's own count; a combined output long enough to lose its head leaves the reading short, and a decline there is unattributable, so the execution is not believed.
A line whose name is empty is the one test's when the process ran exactly one; with several it is quoted and sets nothing aside.

**A decline is believed only where the baseline declined the same way.** A mutation can reach whatever a test reads to decide it cannot measure here, and a decline that appears only under a mutant means the mutation changed what the test did: that is a detection, and the mutation is killed.
Only a test that declined in this run's baseline, for the same reason, is set aside under a mutant.
So the words are the reason and nothing that changes between runs: a temporary path or a process id in them would make every decline under a mutant a new one.

**A survivor rests only on tests that measured.** After a mutant's executions, a test that declined is set aside.
Where every test the run asked about the mutation declined, the mutation was not measured here: it is `not_run` with the reason `declined`, and the report names each test and its words.
Where a test that did not decline reached the mutation and passed, the survivor stands on that test, and the declines are recorded beside it.
A kill is a kill whoever declined: a failing test is evidence, and a decline never hides one.

**An execution with a decline is never stored.** Neither the outcome store nor carry keeps an execution in which any test declined, a survivor that stood on other tests included: an answer established where a test could not measure is the machine's, and read back where it could, it would pass the machine off as the tree.

**A baseline decline excuses that decline, and nothing more.** A test that declined in the baseline is still asked about every mutation it reached, as any test is.
If it declines again in the same words, it is set aside; if the mutation makes it measure instead, what it measured is the answer, a kill or a pass, since the tests then ran and said so.

**What is not a decline.** A line the engine did not ask for — a file it never named, a name that is not a test of that process — is refused as an incoherent execution rather than read, and a decline in a process whose exit the engine did not see end is not believed.

## Consequences

- storage-scout's ext4 fate stops being a survivor and needs no claim: the run says the mutation was not measured on that machine, and why, in the tests' own words.
- A declined mutation is not a finding for the engine, whose exit status says that nothing this machine could measure is wrong; it is counted apart from every other not-run reason and quoted, so a decline is visible rather than a quiet exemption.
- njutest's assurance is the claim that every mutation was noticed, so a run with a declined mutation concludes `INSUFFICIENT` and names the declines as its gap: the engine answers "clean here", and only njutest's `ASSURED` answers "complete".
- A suite can still decline too eagerly; a suite that must measure somewhere turns its own decline into a failure there, as storage-scout's does in CI, and no decline under a mutant that the baseline did not make is believed.
- The trace records each decline, the baseline's on `verify` and each execution's on `mutant-exec`, and engine-audit re-derives each row from them: a decline the baseline did not make must be the row's kill, declines of every test that ran must be a `declined` row, and a row names exactly the declines its execution recorded.

## Alternatives

- **Read what the test prints.** libtest captures the output of a passing test, and asking it not to changes what every test prints; a line in a named file is read the same whatever the harness does with output.
- **Treat an unusually fast pass as a skip.** Speed is not evidence of anything, and it would differ between machines by construction.
- **A crate the suite depends on.** The engine adds nothing to the project under test; a documented variable and a line of text need no dependency.
