<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0031 — A knob is one control started differently

## Status

Accepted, 2026-09-24.
Implemented by `Session::control_perturbed` and the closed sets `execute::Variable`, `Launcher`, and `Schedule` in the engine, by the engine's `perturbed-control` record, by `[repeatable] knobs`, the knob phase, the `knobs` report record, the `environment-dependent` and `environment-dependent-reach` findings and the `knob-not-put` and `knob-not-compared` limitations of njutest, and by the `knobs` layer of `xtask proofaudit`.
Applies [ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md) to the conditions a run holds equal, and [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md) to how it varies them.

## Context

[ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md) asks whether a target's reach is a function of the target, by comparing its baseline with a control run under the same conditions.
A second run under the same conditions cannot see a suite that depends on the conditions themselves.
A test that reads the clock in the local zone, folds a string under the locale, builds a command line from an unquoted temporary path, expects a file in the home directory, relies on the mode a new file gets, measures the terminal, or passes only while another test runs beside it, passes on the machine that wrote it and fails on the next.
Every verdict about such a suite is a verdict about one machine, and the baseline, being one run on one machine, cannot tell.

The contract lets exactly those things differ between machines, so a suite whose answer depends on one of them is a defect in the suite, not in the machine.
Finding one needs a run that differs in that one thing and in nothing else, compared with a run that did not.

## Decision

1. **A knob is one more control of a target that passed, differing from its baseline in one closed thing.** `[repeatable] knobs` names knobs from a closed set, and for each knob a run starts one control of every test binary every test of which passed on the baseline, with that one thing set to a value chosen to differ: `TZ=Australia/Lord_Howe`, whose offset is a half hour and moves by a half hour; `LC_ALL=tr_TR.UTF-8`, whose dotted and dotless `i` break case folding; a temporary directory whose path holds a space and a non-ASCII letter; an empty home directory, keeping cargo's and rustup's; `umask 077`; `COLUMNS=37 LINES=11`; and `--test-threads=1`.
   A binary that did not pass has nothing for a knob to hold, so it is not put under one.

2. **What a knob can set is closed in the engine, and only a control can be started under one.** A perturbation is a value of `execute::Variable`, `Launcher`, and `Schedule`, none of which can name the variables that activate a mutant, record a process's reach, count its steps, or find its libraries.
   It is held in `Conditions`, apart from the `Request` every execution takes, so no mutant execution is ever run under conditions its baseline was not.
   A schedule only libtest understands is refused for a harness that is not libtest rather than handed to a program that never agreed to take it.

3. **A knob this machine cannot put is never counted as held.** The zone has to be in the time zone database, the locale installed, and a shell present for the mask.
   A target run through cargo is not given a temporary directory, which cargo reads itself, nor a home unless cargo's and rustup's are kept, so a failure there would be about the toolchain.
   Each such knob is `knob-not-put`, with the knob, the targets, and why, because a pass under a knob that was never put says nothing.

4. **What a control under a knob established is a closed standing, compared with the baseline as drift compares.** `stable` where it passed the tests its baseline passed and reached the same three unions; `moved` with what each union gained and lost; `broke` with the tests that failed; `passed` where the target records no reach; `uncompared` with drift's reason; `unsettled` where it errored or ran out of time; `not-put` with why.
   A knob that broke a target is `environment-dependent`, a defect, because the suite's answer depends on something the contract lets differ.
   A knob that moved only its reach is `environment-dependent-reach`, counting what rests on the baseline by the rule `unstable-baseline` counts with, since every proof read off that baseline is unfounded where the knob differs.
   A knob whose controls compared nothing is `knob-not-compared`, one per knob and reason.

5. **The engine records a perturbed control apart.** It writes one `perturbed-control` record, naming the variables it was started with and their values, what its launcher ran first, the arguments its schedule added, how it ended, and what became of its reach, and writes no `mutant-exec` or `touch` for it.
   So nothing comparing a control with its baseline under equal conditions ever reads one that ran under different ones.
   What became of its reach is closed too, `not-asked`, `not-read`, `unrecorded`, `unreadable`, or `recorded` with its touch record, each set where the engine decided it, so a reader tells a control never asked to record from one whose record failed without reading a note.

6. **The audit re-derives every standing from the engine's recording alone.** The `knobs` layer of `xtask proofaudit` names the knob each perturbed control was started under by a table of its own of what each knob sets, re-derives its standing from the record and the target's baseline `touch`, and holds it to the report's record, findings, and limitations ([ADR 0004](0004-proof-layers-not-budgets.md), decision 5).
   A control started in a way no knob puts, a knob put twice on one target, a knob the report says was put that the engine never ran, and one the report says was not put that the engine ran, are each a violation.

## Consequences

Each knob is one more run of every passing binary, so `[repeatable] knobs` is empty by default and a run asks for the ones it can afford.
Most CI images install no Turkish locale, so there `locale` is `knob-not-put`; the limitation says so rather than a pass saying nothing.
Two knobs are not put together, so a suite that fails only under two of them at once is not found; each is a single cause a reader can act on, and a pair would be a cause nobody could name.

The working directory and the order of tests are not knobs.
Cargo's contract fixes the first at the package root, and stable libtest cannot reorder the second, so neither is something the contract lets differ between machines.

## Alternatives

Running the whole suite again under a randomised environment would find more and explain less: a failure under ten changed things names none of them, and a pass under them establishes nothing about any one.

Putting knobs on mutant executions as well would multiply the cost by the catalog, and a mutant killed only under a knob is a mutant killed by the environment, which is the class [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md) keeps out of a verdict.
