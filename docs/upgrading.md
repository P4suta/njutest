<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Upgrading njutest

**Status: implemented.** Each section says what changed for somebody who was already running the release before it, and what to do about it.
A change that needs nothing is not listed; the engine's own changes are on [its page](engine/upgrading.md).

## Unreleased

**A verdict is what a sealed run observed.** The mutation phase decides each mutation from the sealed executions of the tests whose sealed controls reached it, and runs it natively only where they establish nothing ([ADR 0046](adr/0046-a-verdict-is-what-a-sealed-run-observed.md), [the assurance contract](assurance-contract.md#what-a-verdict-rests-on)).
What a native run says is a lead: its row raises `unproven-mutant`, answers nothing, and leaves the run `INSUFFICIENT`, exit 2.
A suite whose tests start threads, processes or sockets, or a crate that does not build for `wasm32-wasip1`, leaves what only those tests reach unproven; the finding names every reason.
Install the target with `rustup target add wasm32-wasip1`; `[mutation] seal = false`, or `--no-seal`, builds nothing for it, and every answer is then a lead.

**The report is `schema_version` 3.** Every mutation row carries `evidence`, what its decision rests on ([report v1](report-v1.md#what-a-decision-rests-on)); `accounting.mutants.observers` gains `unproven`; `blind_in` and the findings gain `unproven` and `unproven-mutant`.
A report of version 2 is refused rather than read as sealed.

**Every part carries `repaired`.** One `{ target, again }` per target the part saw move says how many dispositions it ran again against that target ([report v1](report-v1.md#drift)), which is what a merge's `reach-moved` limitation counts; a version 3 part without it, or with counts that are not its moved targets', is refused rather than read as having repaired nothing.

**A part of a divided catalog exits 2.** `PARTIAL` assures nothing on its own, so a job that runs one shard now fails where it passed; `njutest merge` of every part is what concludes.

**What earlier runs established starts cold.** The store of mutation answers is `njutest-mutation-evidence-v3` under `mutants-v3`, each record carrying what it rests on, so nothing the release before kept is read; a stored lead is read back only once sealing is tried and decides nothing, and where sealing decides it the route says `superseded`.
A checkpoint keeps only a kill sealed executions established, with those executions; one the release before left is never read, since a run's identity carries the njutest that decided it.

**`njutest explain` says what each build's answer rests on.** A `SEALED` line names each sealed execution's target, test and what it came to, and an `UNPROVEN` line each reason there is none.
