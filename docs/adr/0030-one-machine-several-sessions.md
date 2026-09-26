<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0030 — One machine, several sessions

## Status

Accepted, 2026-09-24.
Decides how the pre-push gate and whole-workspace runs share the development machine.

## Context

The campaign is developed by several sessions on one machine at once, each in a git worktree of its own, with a merge queue that makes a fresh worktree for every pull request it lands.
On 2026-09-24 three of them and their gates held the load between 5 and 26 on eighteen cores, and the time a change took to land was mostly time spent waiting for tests.

What was measured, as counts:

- The pre-push gate keyed its tree by the worktree's path, and Cargo writes each package's absolute path into its fingerprints.
  Every push from a worktree the gate had not seen was a build from nothing, about 1450 s, and the merge queue's worktrees are always new: at 08:37 two such cold gates ran side by side and the load reached 26.
- One push compiled each core crate about nine times: clippy, rustdoc, xtask, the fuzz workspace, Kani, and four builds that differ only in their feature set.
- Pushing one commit to the hub and then to origin ran the whole gate twice.
- Nothing bounded how many whole-workspace runs shared the machine, so every one of them, and every cargo a toolchain test starts, asked for all eighteen cores.
- macOS evaluated each newly written executable before its first run: 25,100 ms against 13 ms for a second run of the same file, with `syspolicyd` at 57 CPU-hours in under three days.
  Switching the session's applications on under Developer Tools brought the first run to 44–69 ms, and one session's `nextest -p rust-mutants -p njutest` went from 776 s at load 20 to 110 s at load 11.

The gate was a Bash script, and the rule that decides what may run on the machine is a rule the project wants held by a type and a test rather than by a shell's semantics.

## Decision

1. **The gate is `cargo xtask pre-push`.** It is the Bash gate's protocol — exact object, fast-forward only, the tree proven before and after the check, no `GIT_*` variable handed on, a budget that stops the check's whole process group — written in Rust and held by `xtask/tests/pre_push.rs`.
2. **One gate tree per repository.** The key is the repository's common Git directory, so the main checkout and every linked worktree share one tree and one build under the user's cache directory, outside every checkout.
   The tree is checked out in place, so what is compiled again is what the diff from the last push feeds.
3. **One whole-workspace run at a time, by lane.** `cargo xtask slot heavy -- <command>` holds this machine's `heavy` lane for the command's life; the gate holds it from before it touches its tree until it has put the tree back.
   The lane is an operating-system file lock held by the xtask process, opened close-on-exec, so no child — and no daemon a child starts — inherits it, and a holder that dies, however it dies, releases it.
   The work a dead holder started, which the record names by its leader's process id and start time, answers to nobody: the next holder asks its group to stop, kills it after a grace, and goes in only once it has ended, refusing the lane rather than sharing it when it will not end.
   Waiting for it to end on its own was the first form of this decision, and a loop that never ended held the lane for every session on the machine (2026-09-26).
   A waiting run says whom it is waiting for and repeats it; `NJUTEST_SLOT_HELD` lets a run already inside the lane through.
   A narrowed run does not take the lane.
4. **A pass is remembered for an hour**, keyed by the commit, the base the commit-message check reads, and the bytes of the gate binary.

## Consequences

- A push from a new worktree, the merge queue's included, is incremental against the last push rather than cold.
- Two gates never share the machine; the second waits in the open and says for whom, rather than both running at half speed with their bounds failing.
- Pushing the same commit twice runs the gate once.
- The lane is not first-come-first-served: waiting runs poll, and whichever looks first after the holder ends goes next.
  With a handful of sessions that has not mattered; a queue that orders them is the change to make if it does.
- The next run waits for the work's leader, not for every process in its group: the compilation cache's server and Git's file monitor stay in the group that started them, so waiting for the whole group could wait forever.
  A leader that dies before its own children, killed outright or out of memory, therefore lets the next run in while those children still run; that gap is known and left open.
- On Windows nothing reads a process's start time without an unsafe call, so the leader is not recorded, waiting markers are never swept, and a holder killed outright lets the next run in at once; there the lane is only the lock.
- The work a lane admits has no terminal input: it runs apart from the terminal's foreground group, and a program that stopped to read one would hold the lane for ever.
- The gate starts the compilation cache's server itself before the check, with an idle timeout longer than the check's budgets, so stopping the check's group never stops the server every session shares.
- The leader is recorded just after the work starts, so a holder killed in the milliseconds between the two lets the next run in; the gap is known and left open.
- A process that inherits `NJUTEST_SLOT_HELD` from an ancestor holding the lane and then has it taken away waits for that ancestor's leader, which is waiting for it; nothing in this repository strips the variable, so the case is known and left open.
- The gate is one pass: the warming pass that ran the whole check a second time is gone, and the check is bounded by quiet first and a ceiling behind it, as ADR 0026 decides for a mutation.
- A narrowed run can still coincide with a whole one.
  Whether that costs anything is a count to take before widening the lane to it.
- The four feature-set builds, and the Developer Tools setting on a new machine, are separate decisions: the first is a change to `mise.toml`, the second is recorded in [the limitation](../limitations.md#on-macos-measure-what-an-execution-costs-before-measuring-anything-else) and [development](../development.md#one-machine-several-sessions).
